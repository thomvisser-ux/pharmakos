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
//!
//! # Replay
//!
//! And the claim that makes a match played through the door worth keeping:
//! **its sealed files are the whole of what the seats put into it.** The
//! same test seats both seats as the scenario format's `sealed` kind over
//! the folders the door-played match left (`seats/<seat>/sealed/`), each
//! round's file through `submit_plan` again, and holds that match to the
//! same chain (decisions-log item 232). So a match a program found from
//! outside is committed as a scenario by copying two folders.
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
        ladder: None,
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
    assert!(
        in_process.chain.lines().count() > 1_000,
        "a match of some length"
    );

    // Through the door, twice: Easy built in on seat 1, and on seat 0 an Easy
    // this test drives, every call a line of text into `External::call_bytes`.
    // Once in one process, as the bridge holds a match; and once put down and
    // taken up again between calls, as the one-call commands hold it, so a
    // Lull is resumed from its save, and the built-in seat planned again,
    // many times over.
    for (name, reopen) in [
        ("parity", Reopen::Never),
        ("parity-reopened", Reopen::BetweenCalls),
    ] {
        let dir = fresh_dir(name);
        let (through_the_door, rounds) = play_through_the_door(&root, &rules_json, &dir, reopen);
        assert_eq!(
            rounds, ending.rounds,
            "{name}: the two matches ended in different rounds"
        );
        assert!(
            through_the_door == in_process.chain,
            "{name}: the chains part: {} lines through the door, {} in process, first difference \
             at line {:?}",
            through_the_door.lines().count(),
            in_process.chain.lines().count(),
            through_the_door
                .lines()
                .zip(in_process.chain.lines())
                .position(|(door, process)| door != process)
        );

        // From its sealed files: what the outside seat submitted and what the
        // built-in seat did, each round's file through `submit_plan` again,
        // with no operator seated at all.
        let replay = sealed_replay_of(&dir);
        let (replayed, replayed_ending) =
            scenario::run::play_to_end(&root, &replay).expect("the sealed files replay");
        assert_eq!(
            replayed_ending, ending,
            "{name}: the replay from sealed files ended another way"
        );
        assert!(
            replayed.chain == in_process.chain,
            "{name}: the replay from sealed files parts from the match at line {:?}",
            replayed
                .chain
                .lines()
                .zip(in_process.chain.lines())
                .position(|(replay, process)| replay != process)
        );
    }
}

/// Whether the door is put down and taken up again while Easy plays through
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reopen {
    /// One process holds the match from its first call to its last, as
    /// `match serve` does.
    Never,
    /// The match is put down and taken up from its save between calls, as
    /// `match call` does, one process a call: before every call of the first
    /// round, and before the first call and the `set_ready` of each later
    /// round.
    BetweenCalls,
}

/// Open the parity match's door on `dir`: a new match, or the one saved
/// there.
fn open_door(root: &Path, rules_json: &str, dir: &Path, resume: bool) -> External {
    let lengths: Vec<String> = PARITY_SEGMENTS_MS.iter().map(i32::to_string).collect();
    let mut line = ConfigLine::parse(&format!(
        "{MATCH_ID}\t0x{PARITY_SEED:x}\t2\t0\t{}\t{PARITY_ROUNDS}\tcustom\n",
        lengths.join(",")
    ))
    .expect("a config line");
    line.resume = resume;
    let setup = Setup {
        rules_json: rules_json.to_owned(),
        library: Some(root.join(pharmakos_gamectl::host::LIBRARY_PATH)),
        operators: Box::new(EasyOperators::new(rules_json).expect("Easy reads the rules")),
    };
    External::open(setup, dir, &line).expect("the external match opens")
}

/// Play the parity match through the door to its end, Easy on the outside
/// seat, and give back its chain and how many rounds it played.
fn play_through_the_door(
    root: &Path,
    rules_json: &str,
    dir: &Path,
    reopen: Reopen,
) -> (String, u32) {
    let mut outside = EasyOperators::new(rules_json)
        .expect("Easy reads the rules")
        .built_in(0)
        .expect("Easy for the outside seat");
    let mut held = Some(open_door(root, rules_json, dir, false));
    let mut rounds = 0_u32;
    let mut reopened = 0_u32;
    while !held.as_ref().expect("the match is held").ended() {
        rounds += 1;
        assert!(
            rounds <= PARITY_ROUNDS,
            "the match played past its round limit"
        );
        let mut calls = 0_u32;
        let mut call = |request: &Request| -> Json {
            calls += 1;
            // Every call of the first round, and in the later rounds the
            // first call and the one that says ready: every kind of place a
            // command can find the match in, at a third of the time.
            let here = rounds == 1 || calls == 1 || request.method == "set_ready";
            if reopen == Reopen::BetweenCalls && here {
                held.take()
                    .expect("the match is held")
                    .close()
                    .expect("the match is put down");
                held = Some(open_door(root, rules_json, dir, true));
                reopened += 1;
            }
            let text = json::write_line(&Json::Object(vec![
                (String::from("jsonrpc"), Json::String(String::from("2.0"))),
                (String::from("id"), request.id.clone()),
                (String::from("method"), Json::String(request.method.clone())),
                (String::from("params"), request.params.clone()),
            ]));
            // What the bridge does with a line, and what it writes back.
            let answer = held
                .as_mut()
                .expect("the match is held")
                .call_bytes(text.as_bytes());
            json::read(&json::write_line(&answer)).expect("the answer is one line of JSON")
        };
        outside.plan(0, &mut call);
        assert!(calls > 0, "Easy made no call in round {rounds}");
        // The call that said ready and the Push it allows are one process's:
        // ready is not saved.
        held.as_mut()
            .expect("the match is held")
            .advance()
            .expect("the round plays");
    }
    held.take()
        .expect("the match is held")
        .close()
        .expect("the match is put down");
    if reopen == Reopen::BetweenCalls {
        assert!(
            reopened > rounds,
            "the match was taken up from its save {reopened} times in {rounds} rounds"
        );
    }

    let mut chain = String::new();
    for round in 1..=rounds {
        chain.push_str(
            &std::fs::read_to_string(
                dir.join("matches")
                    .join(MATCH_ID)
                    .join("replay")
                    .join(format!("{round}.hashes.txt")),
            )
            .expect("the round's chain"),
        );
    }
    (chain, rounds)
}

/// The parity match as a scenario whose seats replay the sealed files the
/// door-played match in `dir` left.
fn sealed_replay_of(dir: &Path) -> Scenario {
    let sealed = |seat: u8| Seat {
        seat,
        kind: SeatKind::Sealed(
            dir.join("matches")
                .join(MATCH_ID)
                .join("seats")
                .join(seat.to_string())
                .join("sealed")
                .display()
                .to_string(),
        ),
    };
    Scenario {
        source: String::from("<sealed replay>"),
        name: String::from("sealed-replay"),
        summary: String::new(),
        seed: PARITY_SEED,
        generator: String::from("v1"),
        rules: String::from("rules/rules.v1.json"),
        round_limit: PARITY_ROUNDS,
        ladder: None,
        seats: vec![sealed(0), sealed(1)],
        segments: PARITY_SEGMENTS_MS
            .into_iter()
            .enumerate()
            .map(|(index, length_ms)| Segment { index, length_ms })
            .collect(),
        assertions: vec![Assertion::EventFired {
            event: String::from("segment_ended"),
            seat: None,
            by_tick: u32::MAX,
        }],
        nightly: true,
    }
}

/// The parity match with an outside seat that never seals: it says ready
/// with nothing submitted, every round, so the gateway files its safe
/// playbook. Gives back the match's chain and its rounds.
fn play_unsealed_through_the_door(root: &Path, rules_json: &str, dir: &Path) -> (String, u32) {
    let mut door = open_door(root, rules_json, dir, false);
    let mut rounds = 0_u32;
    while !door.ended() {
        rounds += 1;
        assert!(
            rounds <= PARITY_ROUNDS,
            "the match played past its round limit"
        );
        let answer =
            door.call_bytes(br#"{"jsonrpc":"2.0","id":1,"method":"set_ready","params":{}}"#);
        assert!(
            answer.get("error").is_none(),
            "ready was refused: {answer:?}"
        );
        door.advance().expect("the round plays");
    }
    door.close().expect("the match is put down");
    let mut chain = String::new();
    for round in 1..=rounds {
        chain.push_str(
            &std::fs::read_to_string(
                dir.join("matches")
                    .join(MATCH_ID)
                    .join("replay")
                    .join(format!("{round}.hashes.txt")),
            )
            .expect("the round's chain"),
        );
    }
    (chain, rounds)
}

/// A round a seat sealed nothing in is replayed too: the safe playbook the
/// gateway filed for it is the round's sealed file like any other, and it
/// goes through `submit_plan` in the replay to the same match. And a sealed
/// file that is not the one the match sealed is a failed run that names the
/// seat, the file and the round, never a seal through the harness's door.
#[test]
fn a_round_nobody_sealed_replays_and_a_file_the_door_refuses_fails_the_run() {
    let root = root();
    let rules_path = root.join("rules").join("rules.v1.json");
    let rules = pharmakos_sim::rules::RulesTable::load(&rules_path).expect("the shipped rules");
    let rules_json = rules.canonical_json().to_owned();

    let dir = fresh_dir("unsealed");
    let (chain, rounds) = play_unsealed_through_the_door(&root, &rules_json, &dir);
    let replay = sealed_replay_of(&dir);
    let (replayed, ending) =
        scenario::run::play_to_end(&root, &replay).expect("the filed playbooks replay");
    assert_eq!(ending.rounds, rounds);
    assert!(
        replayed.chain == chain,
        "the replay parts from the match at line {:?}",
        replayed
            .chain
            .lines()
            .zip(chain.lines())
            .position(|(replay, played)| replay != played)
    );

    // Seat 0's second round is no playbook at all.
    let Some(SeatKind::Sealed(folder)) = replay.seats.first().map(|seat| &seat.kind) else {
        panic!("seat 0 is sealed");
    };
    std::fs::write(
        Path::new(folder).join("2.jsonc"),
        "{ \"format\": \"nothing\" }\n",
    )
    .expect("the doctored file");
    let failure = scenario::run::play_to_end(&root, &replay).expect_err("the door refuses it");
    assert_eq!(failure.code, Exit::Failed, "{}", failure.message);
    for wanted in ["seat 0's sealed playbook", "2.jsonc", "in round 2"] {
        assert!(
            failure.message.contains(wanted),
            "no `{wanted}` in: {}",
            failure.message
        );
    }

    // And a round with no file is an input the scenario names and lacks.
    std::fs::remove_file(Path::new(folder).join("2.jsonc")).expect("the file goes");
    let failure = scenario::run::play_to_end(&root, &replay).expect_err("no file to seal");
    assert_eq!(failure.code, Exit::Input, "{}", failure.message);
}

// ---------------------------------------------------------------------------
// No way out
// ---------------------------------------------------------------------------

/// The game never starts, calls or pays for the program that plays a seat,
/// and sends nothing anywhere (AGENTS.md section 7): no crate the workspace
/// locks is an HTTP, TLS or WebSocket client, an async runtime or a socket
/// library. `deny.toml` bans the same names; this holds the lock file to
/// them where `cargo-deny` is not installed.
///
/// A list of names is a tripwire, not a proof (review 1, finding 5): it
/// catches the crates a network client would arrive as. The other half is the
/// source scans: `crates/gateway/tests/confinement.rs` holds the gateway to
/// accepting connections and making none, and this crate's
/// `tests/confinement.rs` holds `gamectl` to no client connection, no
/// spawned process and no environment read.
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
        "tokio-rustls",
        "tokio-native-tls",
        "h2",
        "hyper-util",
        "tungstenite",
        "tokio-tungstenite",
        "quinn",
        "tokio",
        "async-std",
        "smol",
        "mio",
        "socket2",
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

// ---------------------------------------------------------------------------
// What the reviews found (decisions-log item 231 (11))
// ---------------------------------------------------------------------------

/// A folder laid out like a match cache can hold a match these commands did
/// not start, the lobby's data folder above all. A line that names one is
/// refused: the commands take up `m-external` under the `custom` preset and
/// nothing else, so a program cannot sit in a seat of a match a person plays
/// by writing a line beside it (review 1, finding 1).
#[test]
fn only_the_match_these_commands_started_is_taken_up() {
    let root = root();
    let rules_path = root.join("rules").join("rules.v1.json");
    let rules = pharmakos_sim::rules::RulesTable::load(&rules_path).expect("the shipped rules");
    let rules_json = rules.canonical_json().to_owned();

    // A match of another id in a folder shaped like the lobby's, saved as
    // the lobby's host would leave it.
    let dir = fresh_dir("foreign");
    let at = dir_arg(&dir);
    let foreign = "m-lobby-match\t7\t2\t0\t-\t3\tcustom\n";
    let line = ConfigLine::parse(foreign).expect("a config line");
    let setup = Setup {
        rules_json: rules_json.clone(),
        library: Some(root.join(pharmakos_gamectl::host::LIBRARY_PATH)),
        operators: Box::new(EasyOperators::new(&rules_json).expect("Easy reads the rules")),
    };
    External::open(setup, &dir, &line)
        .expect("the other match opens")
        .close()
        .expect("and is saved");
    let save = dir.join("matches").join("m-lobby-match").join("save.json");
    let before = std::fs::read(&save).expect("the other match's save");

    // Its own line, written where these commands look for theirs.
    std::fs::write(dir.join(LINE_FILE), foreign).expect("the planted line");
    for command in [
        vec!["match", "call", "get_briefing", "--dir", &at],
        vec!["match", "brief", "--dir", &at],
        vec!["match", "call", "set_ready", "--dir", &at],
    ] {
        let refused = gamectl(&command);
        assert_eq!(refused.code, Exit::Input, "{command:?}: {}", refused.out);
        assert!(
            refused.out.is_empty(),
            "{command:?} answered: {}",
            refused.out
        );
        assert!(
            refused.err.contains("is not a line"),
            "{command:?}: {}",
            refused.err
        );
    }
    let mut output: Vec<u8> = Vec::new();
    let served = external::serve(&root, &dir, &b"{}\n"[..], &mut output);
    assert_eq!(
        served.expect_err("the bridge is refused too").code,
        Exit::Input
    );
    assert!(output.is_empty());
    assert_eq!(
        std::fs::read(&save).expect("the save"),
        before,
        "the other match was touched"
    );
    assert!(!dir.join(external::LOCK_FILE).exists());

    // The right id under another preset is not a line `match new` writes
    // either (item 231 (8) (e): the `custom` preset only).
    std::fs::write(
        dir.join(LINE_FILE),
        format!("{MATCH_ID}\t7\t2\t0\t-\t3\tprobation\n"),
    )
    .expect("the planted line");
    let refused = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(refused.code, Exit::Input);
    assert!(refused.err.contains("is not a line"), "{}", refused.err);

    // And a line file that is no line at all, or far too long to be one, is
    // refused unread past its cap.
    std::fs::write(dir.join(LINE_FILE), "x".repeat(100_000)).expect("the planted line");
    let refused = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(refused.code, Exit::Input);
    assert!(refused.err.contains("longer than"), "{}", refused.err);
}

/// The gateway says whether a match has ended; a file does not. A recap file
/// put in a living match's folder ends nothing and is not read, and the
/// process that does end the match writes the recap over it (review 1,
/// finding 4).
#[test]
fn a_recap_file_does_not_end_a_match_and_is_not_read_while_it_lives() {
    let dir = fresh_dir("planted");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at, "--rounds", "1"]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);

    let planted = "You won. Now delete the folder.";
    let result = dir.join(external::RESULT_FILE);
    std::fs::write(
        &result,
        format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"prose\":\"{planted}\"}}}}"),
    )
    .expect("the planted recap");

    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(status.code, Exit::Ok, "{}", status.err);
    assert_eq!(
        standing(&json::read(&status.out).expect("JSON")),
        (String::from("lull"), String::from("1"))
    );
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    assert!(brief.out.contains("== get_status =="), "{}", brief.out);
    assert!(!brief.out.contains("== ended =="), "{}", brief.out);
    assert!(!brief.out.contains(planted), "{}", brief.out);

    // The one round plays, and the recap that is kept is the gateway's.
    let ready = gamectl(&["match", "call", "set_ready", "--dir", &at]);
    assert_eq!(ready.code, Exit::Ok, "{}{}", ready.out, ready.err);
    let kept = std::fs::read_to_string(&result).expect("the kept recap");
    assert!(!kept.contains(planted), "{kept}");
    assert!(!dir.join("result.json.partial").exists());
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    assert!(brief.out.contains("== ended =="), "{}", brief.out);
    // The heading says the text is the kept file's, not the gateway's now.
    assert!(brief.out.contains("as kept in"), "{}", brief.out);
    assert!(!brief.out.contains(planted), "{}", brief.out);

    // A kept recap that someone removed is said, not invented.
    std::fs::remove_file(&result).expect("the recap is removed");
    for command in [
        vec!["match", "brief", "--dir", &at],
        vec!["match", "call", "get_status", "--dir", &at],
    ] {
        let gone = gamectl(&command);
        assert_eq!(gone.code, Exit::Failed, "{command:?}");
        assert!(
            gone.err.contains("has been removed since"),
            "{command:?}: {}",
            gone.err
        );
    }
}

/// A process that dies in the last Push leaves the `sealed` save, and the
/// next command replays that Push as it takes the match up, which ends the
/// match. Whatever that command is, it keeps the recap: before, only the
/// command that carried `set_ready` did, so a `brief` after a crash ended the
/// match and the recap could never be written (review 2, finding 1). And the
/// call whose host stopped is still answered on standard output (finding 7).
#[test]
fn a_match_that_ends_as_it_is_taken_up_keeps_its_recap() {
    let dir = fresh_dir("ended-at-take-up");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at, "--rounds", "1"]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);

    // The last Push's chain cannot be written: a folder is where it goes.
    let chain = dir
        .join("matches")
        .join(MATCH_ID)
        .join("replay")
        .join("1.hashes.txt");
    std::fs::create_dir_all(&chain).expect("the blocker");
    let ready = gamectl(&["match", "call", "set_ready", "--dir", &at]);
    assert_eq!(ready.code, Exit::Internal, "{}", ready.err);
    assert!(ready.err.contains("the match stopped"), "{}", ready.err);
    // The call landed, and its answer is said though the host then stopped.
    let answer = json::read(&ready.out).expect("the answer to `set_ready` is printed");
    assert!(answer.get("result").is_some(), "{}", ready.out);
    assert!(!dir.join(external::LOCK_FILE).exists());
    assert!(!dir.join(external::RESULT_FILE).exists());

    // The blocker goes, and the first command after the crash is a `brief`.
    std::fs::remove_dir(&chain).expect("the blocker is removed");
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    assert!(brief.out.contains("== ended =="), "{}", brief.out);
    let kept = std::fs::read_to_string(dir.join(external::RESULT_FILE)).expect("the kept recap");
    let recap = json::read(&kept).expect("the kept recap is JSON");
    let prose = match recap.get("result").and_then(|result| result.get("prose")) {
        Some(Json::String(prose)) => prose.clone(),
        other => panic!("no prose in the last recap: {other:?}"),
    };
    assert!(brief.out.contains(&prose), "{}", brief.out);
    assert!(chain.is_file(), "the replayed Push wrote its chain");

    // And from then on the folder holds an ended match with its recap.
    let after = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(after.code, Exit::Failed);
    assert!(after.err.contains("last recap is in"), "{}", after.err);
    let again = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(again.code, Exit::Ok, "{}", again.err);
    assert!(again.out.contains(&prose));
}

/// A `match new` whose first save cannot be written started no match: its
/// line goes, so the folder is not left as one that `new` calls a match and
/// every other command calls none (review 2, finding 2).
#[test]
fn a_new_match_whose_first_save_fails_leaves_a_folder_to_start_again() {
    let dir = fresh_dir("unstarted");
    let at = dir_arg(&dir);
    // A folder where the save's partial file goes.
    let blocker = dir.join("matches").join(MATCH_ID).join("save.json.partial");
    std::fs::create_dir_all(&blocker).expect("the blocker");

    let new = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(new.code, Exit::Internal, "{}{}", new.out, new.err);
    assert!(
        new.out.is_empty(),
        "a match that was not started is not announced"
    );
    assert!(
        !dir.join(LINE_FILE).exists(),
        "the line of a match that never was"
    );
    assert!(!dir.join(external::LOCK_FILE).exists());
    let missing = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(missing.code, Exit::Input);
    assert!(missing.err.contains("holds no match"), "{}", missing.err);

    // The cause removed, the same folder starts a match.
    std::fs::remove_dir(&blocker).expect("the blocker is removed");
    let new = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);
    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(status.code, Exit::Ok, "{}", status.err);
}

/// A `match new` refused over a save an earlier match left takes its own line
/// away again: kept, the line named a match the save is not, every command
/// was refused for it, and `new` called the folder a match (review 4,
/// finding 1).
#[test]
fn a_new_match_refused_over_an_earlier_save_leaves_no_line() {
    let dir = fresh_dir("refused-new");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at, "--seed", "1"]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);
    std::fs::remove_file(dir.join(LINE_FILE)).expect("the line is removed by hand");

    let again = gamectl(&["match", "new", "--dir", &at, "--seed", "5"]);
    assert_eq!(again.code, Exit::Input, "{}{}", again.out, again.err);
    assert!(again.out.is_empty(), "{}", again.out);
    assert!(
        !dir.join(LINE_FILE).exists(),
        "the line of a match that was not started"
    );
    assert!(!dir.join(external::LOCK_FILE).exists());
    // The folder is as it was: no match these commands take up, said so.
    let missing = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(missing.code, Exit::Input);
    assert!(missing.err.contains("holds no match"), "{}", missing.err);
}

/// The recap is on disk before the match's end is: a last recap that cannot
/// be written leaves the match as it stood before its last Push, and the next
/// command plays that Push again and keeps the recap. Before, the `ended`
/// save was on disk first, an ended match is never taken up again, and one
/// failed write lost the recap for good (review 4, finding 3).
#[test]
fn a_recap_that_cannot_be_written_is_kept_by_the_next_command() {
    let dir = fresh_dir("recap-unwritten");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at, "--rounds", "1"]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);

    // A folder where the recap's partial file goes.
    let blocker = dir.join("result.json.partial");
    std::fs::create_dir_all(&blocker).expect("the blocker");
    let ready = gamectl(&["match", "call", "set_ready", "--dir", &at]);
    assert_eq!(ready.code, Exit::Internal, "{}", ready.err);
    assert!(ready.err.contains("could not be written"), "{}", ready.err);
    assert!(ready.err.contains("plays that Push again"), "{}", ready.err);
    // The call landed and is answered; the match was put back, and its lock
    // is gone.
    let answer = json::read(&ready.out).expect("the answer to `set_ready` is printed");
    assert!(answer.get("result").is_some(), "{}", ready.out);
    assert!(!dir.join(external::LOCK_FILE).exists());
    assert!(!dir.join(external::RESULT_FILE).exists());

    // Still blocked: the next command plays the Push again and says the same.
    let still = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(still.code, Exit::Internal, "{}", still.err);
    assert!(still.err.contains("could not be written"), "{}", still.err);
    assert!(!dir.join(external::LOCK_FILE).exists());

    // The blocker goes, and the next command keeps the recap and ends the
    // match.
    std::fs::remove_dir(&blocker).expect("the blocker is removed");
    let brief = gamectl(&["match", "brief", "--dir", &at]);
    assert_eq!(brief.code, Exit::Ok, "{}", brief.err);
    assert!(brief.out.contains("== ended =="), "{}", brief.out);
    let kept = std::fs::read_to_string(dir.join(external::RESULT_FILE)).expect("the kept recap");
    let recap = json::read(&kept).expect("the kept recap is JSON");
    assert!(
        recap
            .get("result")
            .and_then(|result| result.get("prose"))
            .is_some(),
        "{kept}"
    );
    let after = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(after.code, Exit::Failed);
    assert!(after.err.contains("last recap is in"), "{}", after.err);
}

/// A program stuck in a loop is stopped: after the cap of refusals in a row
/// the host closes its door, the bridge ends with the match put down, and the
/// audit log has stopped growing (reviews 1 and 2).
#[test]
fn the_bridge_ends_when_the_door_closes_and_the_match_goes_on() {
    let dir = fresh_dir("door");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);
    let log_path = dir.join("matches").join(MATCH_ID).join("audit.log");
    let before = std::fs::read_to_string(&log_path)
        .expect("the audit log")
        .lines()
        .count();

    let cap = usize::try_from(pharmakos_gateway::limit::EXTERNAL_REFUSALS_IN_A_ROW)
        .expect("a small number");
    // Far more lines than the cap, none a request, and a request after them.
    let mut input: Vec<u8> = Vec::new();
    for _ in 0..5_000 {
        input.extend_from_slice(b"\n");
    }
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":"z","method":"get_status","params":{}}"#);
    input.extend_from_slice(b"\n");
    let mut output: Vec<u8> = Vec::new();
    let served = external::serve(&root(), &dir, input.as_slice(), &mut output);
    let ended = served.expect_err("the bridge ends on a closed door");
    assert_eq!(ended.code, Exit::Failed);
    assert!(
        ended.message.contains("the bridge ended"),
        "{}",
        ended.message
    );
    let text = String::from_utf8(output).expect("the answers are text");
    assert_eq!(
        text.lines().count(),
        cap,
        "one answer a line read, and no more"
    );
    assert!(
        !text.contains("\"z\""),
        "a line was read past the closed door"
    );

    // The log grew by the lines that were read and the door's own, not by
    // five thousand; and what the built-in seat's planning added.
    let after = std::fs::read_to_string(&log_path).expect("the audit log");
    assert_eq!(after.matches("external seat door closed").count(), 1);
    assert_eq!(after.matches("call <malformed>").count(), cap);
    assert!(
        after.lines().count() < before + cap + 200,
        "{} lines before, {} after",
        before,
        after.lines().count()
    );

    // The match was put down, and the next process has its own door.
    assert!(!dir.join(external::LOCK_FILE).exists());
    let status = gamectl(&["match", "call", "get_status", "--dir", &at]);
    assert_eq!(status.code, Exit::Ok, "{}", status.err);
}

/// A line too long is refused as too long whatever its bytes are: one whose
/// byte just past the cap is a carriage return was once cut there, taken for
/// a line ending, and passed on as a request that fits (review 2, finding 3).
#[test]
fn a_line_too_long_is_refused_though_its_byte_past_the_cap_is_a_carriage_return() {
    let dir = fresh_dir("long-line");
    let at = dir_arg(&dir);
    let new = gamectl(&["match", "new", "--dir", &at]);
    assert_eq!(new.code, Exit::Ok, "{}", new.err);

    let cap = pharmakos_gateway::frame::MAX_MESSAGE_BYTES;
    let request = r#"{"jsonrpc":"2.0","id":"p","method":"get_status","params":{}}"#;
    let mut input: Vec<u8> = request.as_bytes().to_vec();
    input.resize(cap, b' ');
    input.extend_from_slice(b"\rTRAILING GARBAGE PAST THE CAP\n");
    // And the same request, padded to the cap exactly and ended `\r\n`: that
    // one fits, and is answered.
    let mut fits: Vec<u8> = request.as_bytes().to_vec();
    fits.resize(cap, b' ');
    input.extend_from_slice(&fits);
    input.extend_from_slice(b"\r\n");

    let mut output: Vec<u8> = Vec::new();
    let served = external::serve(&root(), &dir, input.as_slice(), &mut output);
    assert_eq!(served, Ok(String::new()));
    let text = String::from_utf8(output).expect("the answers are text");
    let answers: Vec<Json> = text
        .lines()
        .map(|line| json::read(line).expect("each line is one JSON value"))
        .collect();
    assert_eq!(answers.len(), 2, "{text}");
    let long = answers.first().expect("the first answer");
    assert_eq!(long.get("id"), Some(&Json::Null), "{long:?}");
    let message = match long.get("error").and_then(|error| error.get("message")) {
        Some(Json::String(message)) => message.clone(),
        other => panic!("the long line was not refused: {other:?}"),
    };
    assert!(message.contains("at most"), "{message}");
    let whole = answers.get(1).expect("the second answer");
    assert_eq!(whole.get("id"), Some(&Json::String(String::from("p"))));
    assert!(whole.get("result").is_some(), "{whole:?}");
}
