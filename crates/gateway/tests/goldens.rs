// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The gateway's goldens: the fog filter, the segment digest, the audit log and
//! the handshake.
//!
//! Each test writes `actual.<ext>` under `<target>/golden/gateway/<case>/` and
//! compares it with `tests/golden/gateway/<case>/expected.<ext>`;
//! `cargo xtask ci`'s `golden` step compares the same pair, and
//! `cargo xtask golden --bless` accepts a move. `tests/golden/gateway/README.md`
//! says what a diff in each of the four means.
//!
//! Every file here is text, one record per line, LF endings, with a trailing
//! newline -- `tests/golden/README.md` rule 3, enforced by the `golden` step
//! because nothing else in the toolchain can be.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gateway::audit::Outcome;
use pharmakos_gateway::feed::{Event, Kind, digests};
use pharmakos_gateway::fog::{Audience, FogFilter, FogPolicy, KnownVoxels, Viewer};
use pharmakos_gateway::handshake::{self, Policy};
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::Surface;
use pharmakos_gateway::time::MatchTime;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use std::fs;
use std::path::{Path, PathBuf};

const MATCH: &str = "m-0001";
const SEED: u64 = 0x00ca_5cad_ed00_0001;
const PORT: u16 = 9500;

// ---------------------------------------------------------------------------
// Paths and comparison
// ---------------------------------------------------------------------------

/// The workspace root: this crate is `<root>/crates/gateway`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("crates/gateway sits two levels below the workspace root"))
        .to_path_buf()
}

/// The cargo target directory. `CARGO_TARGET_TMPDIR` is `<target>/tmp`, and it
/// is the only way a test can find the target directory that also honours
/// `CARGO_TARGET_DIR` -- which every agent worktree sets to its own lane.
fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .unwrap_or_else(|| panic!("CARGO_TARGET_TMPDIR always has a parent"))
        .to_path_buf()
}

/// The one line ending every golden in this tree uses.
const NEWLINE: char = '\n';

/// Append one record and its newline.
///
/// A helper rather than `push_str(&format!(..))`, which
/// `clippy::format_push_string` denies -- and `write!` into a `String` cannot
/// fail, so a helper reads better than a discarded `Result` at every call site.
fn row(out: &mut String, record: &str) {
    out.push_str(record);
    out.push(NEWLINE);
}

/// Write the fresh output and compare it with the committed golden.
fn check(case: &str, extension: &str, contents: &str) {
    assert!(!contents.contains('\r'), "{case}: LF endings only");
    assert!(contents.ends_with('\n'), "{case}: a trailing newline");

    let fresh = target_dir().join("golden").join("gateway").join(case);
    fs::create_dir_all(&fresh)
        .unwrap_or_else(|error| panic!("creating {}: {error}", fresh.display()));
    fs::write(fresh.join(format!("actual.{extension}")), contents)
        .unwrap_or_else(|error| panic!("writing {case}'s fresh output: {error}"));

    let golden = workspace_root()
        .join("tests")
        .join("golden")
        .join("gateway")
        .join(case)
        .join(format!("expected.{extension}"));
    let Ok(expected) = fs::read_to_string(&golden) else {
        panic!(
            "no committed golden at {}. Run `cargo xtask golden --bless` once the fresh output \
             is right.",
            golden.display()
        );
    };
    assert_eq!(
        expected, contents,
        "the {case} golden moved. tests/golden/gateway/README.md says what a diff here means; \
         `cargo xtask golden --bless` accepts it, and the pull request has to say which \
         behaviour changed."
    );
}

fn rules() -> RulesTable {
    RulesTable::load(&workspace_root().join("rules").join("rules.v1.json"))
        .expect("the shipped rules table")
}

// ---------------------------------------------------------------------------
// 1. The fog filter
// ---------------------------------------------------------------------------

/// One tick's event set, replayed through every viewer's filter under every
/// policy the match can be in.
///
/// This is skeleton plan T9's "a fog-filter golden replaying one tick's event
/// set through each seat's filter", widened by one axis: the same events are
/// also run through the casual no-fog policy and through an eliminated seat's
/// view, because those are the two ways a *policy* changes what a seat sees
/// without any token changing (decisions-log item 26).
#[test]
fn the_fog_filter_golden() {
    let seat0 = SeatId::new(0);
    let seat1 = SeatId::new(1);
    let seen = Voxel { x: 12, y: 8, z: 33 };
    let unseen = Voxel {
        x: 300,
        y: 300,
        z: 40,
    };

    let events: Vec<(&str, Audience)> = vec![
        ("phase_changed", Audience::Public),
        (
            "commander_moved",
            Audience::World {
                owner: Some(seat0),
                at: seen,
            },
        ),
        (
            "beacon_placed",
            Audience::World {
                owner: Some(seat1),
                at: seen,
            },
        ),
        (
            "ore_delivered",
            Audience::World {
                owner: Some(seat1),
                at: unseen,
            },
        ),
        (
            "crater_formed",
            Audience::World {
                owner: None,
                at: unseen,
            },
        ),
        ("draft_saved", Audience::Private(seat0)),
        ("notes_saved", Audience::Private(seat1)),
    ];

    // Seat 0 has seen one voxel and nothing else. Seat 1 has seen nothing: it
    // is told about its own assets because they are its own, not because it can
    // see them.
    let mut vision = KnownVoxels::new();
    vision.see(seat0, &seen);

    let viewers: Vec<(&str, Viewer)> = vec![
        ("seat.0", Viewer::Seat(seat0)),
        ("seat.1", Viewer::Seat(seat1)),
        ("spectator", Viewer::Spectator { nofog: false }),
        ("spectator.nofog", Viewer::Spectator { nofog: true }),
        ("admin", Viewer::Admin),
    ];

    let mut eliminated = FogPolicy::fogged();
    eliminated.eliminate(seat0);
    let mut ended = FogPolicy::fogged();
    ended.end_match();
    let policies: Vec<(&str, FogPolicy)> = vec![
        ("fogged", FogPolicy::fogged()),
        ("casual-nofog", FogPolicy::casual()),
        ("seat0-eliminated", eliminated),
        ("match-ended", ended),
    ];

    let mut out = String::from(
        "# One tick's events through every viewer's fog filter (spec section 12; item 26).\n\
         # Seat 0 has seen voxel (12,8,33) and nothing else; seat 1 has seen nothing.\n\
         # policy\tviewer\tevent\towner\tverdict\n",
    );
    for (policy_name, policy) in &policies {
        let filter = FogFilter::new(policy, &vision);
        for (viewer_name, viewer) in &viewers {
            for (kind, audience) in &events {
                let owner = match audience {
                    Audience::Public => String::from("-"),
                    Audience::Private(seat) => format!("seat.{}", seat.raw()),
                    Audience::World { owner, .. } => owner
                        .map_or_else(|| String::from("-"), |seat| format!("seat.{}", seat.raw())),
                };
                let verdict = if filter.visible(*viewer, audience) {
                    "shown"
                } else {
                    "hidden"
                };
                row(
                    &mut out,
                    &format!("{policy_name}\t{viewer_name}\t{kind}\t{owner}\t{verdict}"),
                );
            }
        }
    }
    check("fog_one_tick", "fog.txt", &out);
}

// ---------------------------------------------------------------------------
// 2. The segment digest
// ---------------------------------------------------------------------------

/// The 60-second digest cadence, with the per-kind counts decisions-log item 97
/// owes the scenario vocabulary.
#[test]
fn the_segment_digest_golden() {
    let at = |ms: i32, kind: &str| Event {
        at_ms: Ms::new(ms),
        kind: Kind::new(kind).expect("a well formed kind"),
        text: String::from("."),
        audience: Audience::Public,
    };
    // Three minutes of a segment: a busy first minute, a quiet second, a
    // closing third. The quiet minute is the case worth having a golden for --
    // a client rendering a timeline needs the gap to be there.
    let events = vec![
        at(0, "phase_changed"),
        at(1_500, "commander_moved"),
        at(4_000, "commander_moved"),
        at(12_000, "beacon_placed"),
        at(31_000, "ore_delivered"),
        at(31_000, "commander_moved"),
        at(59_999, "power_settled"),
        at(121_000, "structure_built"),
        at(179_000, "phase_changed"),
    ];

    let mut out = String::from(
        "# get_segment_feed's digests: one per 60 s of game time (spec section 12),\n\
         # each carrying a per-kind count a scenario file can assert on (item 97).\n\
         # from_ms\tto_ms\ttotal\tcounts\tprose\n",
    );
    for digest in digests(&events) {
        let counts = digest
            .counts
            .iter()
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect::<Vec<String>>()
            .join(" ");
        let counts = if counts.is_empty() {
            String::from("-")
        } else {
            counts
        };
        row(
            &mut out,
            &format!(
                "{}\t{}\t{}\t{counts}\t{}",
                digest.from_ms.raw(),
                digest.to_ms.raw(),
                digest.total(),
                digest.text
            ),
        );
    }
    check("segment_digest", "digest.txt", &out);
}

// ---------------------------------------------------------------------------
// 3. The audit log
// ---------------------------------------------------------------------------

/// A scripted session's audit log, end to end through the surface.
///
/// Every step of the pipeline appears once: an upgrade, a served call, a
/// forbidden scope, a closed phase, an unknown method, a method T13 has yet to
/// fill, a rate limit, and a call with no token at all.
#[test]
fn the_audit_log_golden() {
    let mut surface = Surface::new(
        MATCH,
        SEED,
        rules(),
        FogPolicy::fogged(),
        &[SeatId::new(0), SeatId::new(1)],
    )
    .expect("a match id");
    surface.set_limits(pharmakos_gateway::limit::Limits {
        per_tick: 3,
        per_window: 100,
        window_ticks: 200,
    });
    surface.set_time(MatchTime {
        tick: Tick::new(10),
        phase: Phase::Lull,
        phase_remaining_ms: Ms::new(174_000),
        segment_length_ms: Ms::new(180_000),
        round: 1,
    });

    let full = ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]);
    let (seat0, _) = surface
        .tokens()
        .mint(Subject::Seat(SeatId::new(0)), MATCH, full, Tick::ZERO)
        .expect("minted");
    let (seat1, _) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(1)),
            MATCH,
            ScopeSet::of(&[Scope::Observe]),
            Tick::ZERO,
        )
        .expect("minted");

    // The upgrade a session would have logged.
    surface.audit().record(
        Tick::new(10),
        Some(Subject::Seat(SeatId::new(0))),
        Some(pharmakos_gateway::token::Handle::from_raw(1)),
        "upgrade",
        Outcome::Ok,
    );

    let call = |surface: &mut Surface, token: Option<&Token>, method: &str, params: &str| {
        let text = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
        let request = rpc::parse(&text).expect("well formed");
        let _: Json = surface.call(token, &request, &pharmakos_gateway::fog::Blind);
    };

    call(&mut surface, Some(&seat0), "get_status", "{}");
    call(
        &mut surface,
        Some(&seat0),
        "save_notes",
        r#"{"notes":"walk east"}"#,
    );
    call(
        &mut surface,
        Some(&seat1),
        "save_notes",
        r#"{"notes":"no plan scope"}"#,
    );
    call(&mut surface, Some(&seat0), "connect", "{}");

    let mut time = surface.time();
    time.tick = Tick::new(11);
    surface.set_time(time);
    call(&mut surface, Some(&seat0), "get_briefing", "{}");
    call(&mut surface, None, "get_status", "{}");

    // The Push: planning is closed.
    let mut time = surface.time();
    time.tick = Tick::new(12);
    time.phase = Phase::Push;
    surface.set_time(time);
    call(
        &mut surface,
        Some(&seat0),
        "save_notes",
        r#"{"notes":"too late"}"#,
    );

    // And the limiter, at three calls a tick.
    for _ in 0..4 {
        call(&mut surface, Some(&seat1), "get_status", "{}");
    }

    let mut out = String::from(
        "# One scripted session through the gateway surface.\n\
         # Handles name tokens; no token, no params and no result ever reach this log.\n",
    );
    out.push_str(&surface.audit().render());
    check("audit_log", "audit.txt", &out);
}

// ---------------------------------------------------------------------------
// 4. The handshake
// ---------------------------------------------------------------------------

/// The bearer token every handshake case carries: well formed, and belonging to
/// no match, because these cases stop at [`handshake::review`] and never reach a
/// token store.
fn sample_token() -> String {
    "0".repeat(63) + "1"
}

/// The header lines of a conforming upgrade, plus anything `extra` adds.
fn conforming(extra: &[&str]) -> Vec<String> {
    let mut lines = vec![
        format!("Host: 127.0.0.1:{PORT}"),
        String::from("Upgrade: websocket"),
        String::from("Connection: Upgrade"),
        String::from("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="),
        String::from("Sec-WebSocket-Version: 13"),
        format!("Authorization: Bearer {}", sample_token()),
    ];
    for line in extra {
        lines.push((*line).to_owned());
    }
    lines
}

/// A conforming upgrade with one header line replaced.
fn replaced(index: usize, line: &str) -> Vec<String> {
    let mut lines = conforming(&[]);
    if let Some(slot) = lines.get_mut(index) {
        line.clone_into(slot);
    }
    lines
}

/// A conforming upgrade with one header line missing.
fn without(index: usize) -> Vec<String> {
    let mut lines = conforming(&[]);
    if index < lines.len() {
        lines.remove(index);
    }
    lines
}

/// Header lines into an HTTP request.
fn handshake_text(lines: &[String]) -> String {
    let mut text = String::from("GET /seat HTTP/1.1\r\n");
    for line in lines {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text.push_str("\r\n");
    text
}

/// The status and either the accept value or the reason.
fn reviewed(text: &str, policy: &Policy) -> (u16, String) {
    match handshake::review(text, policy) {
        Ok(upgrade) => (101, upgrade.accept),
        Err(refusal) => (refusal.status(), String::from(refusal.reason())),
    }
}

/// Every case the golden covers, in the order it is written.
fn handshake_cases() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("conforming", conforming(&[])),
        ("ipv6-loopback", replaced(0, &format!("Host: [::1]:{PORT}"))),
        (
            "localhost-name",
            replaced(0, &format!("Host: localhost:{PORT}")),
        ),
        (
            "connection-list",
            replaced(2, "Connection: keep-alive, Upgrade"),
        ),
        (
            "foreign-host",
            replaced(0, &format!("Host: pharmakos.example:{PORT}")),
        ),
        ("other-port", replaced(0, "Host: 127.0.0.1:9501")),
        ("no-host", without(0)),
        (
            "browser-origin",
            conforming(&["Origin: http://evil.example"]),
        ),
        (
            "loopback-origin",
            conforming(&[&format!("Origin: http://127.0.0.1:{PORT}")]),
        ),
        ("no-upgrade", without(1)),
        ("no-connection-upgrade", without(2)),
        ("short-key", replaced(3, "Sec-WebSocket-Key: Zm9v")),
        ("no-key", without(3)),
        ("version-8", replaced(4, "Sec-WebSocket-Version: 8")),
        ("no-version", without(4)),
        ("no-authorization", without(5)),
        (
            "basic-authorization",
            replaced(5, "Authorization: Basic c2VhdDpwdw=="),
        ),
        ("short-token", replaced(5, "Authorization: Bearer 00ff")),
        (
            "duplicate-key",
            conforming(&["Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="]),
        ),
        ("smuggled-header", conforming(&["X-Smuggled : 1"])),
    ]
}

/// Every upgrade the gateway will and will not accept, with the status and the
/// reason it answers.
#[test]
fn the_handshake_golden() {
    let policy = Policy::loopback(PORT);
    let mut out = String::from(
        "# Every opening handshake the gateway will and will not accept\n\
         # (RFC 6455 section 4.2.1; spec section 12's Host and Origin checks).\n\
         # The key is RFC 6455 section 1.3's worked example, so the accept value is the\n\
         # standard's own: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\n\
         # case\tstatus\tresult\n",
    );
    for (name, lines) in handshake_cases() {
        let (status, result) = reviewed(&handshake_text(&lines), &policy);
        row(&mut out, &format!("{name}\t{status}\t{result}"));
    }

    // Two cases that are not about headers at all, and so do not fit the table.
    for (name, text) in [
        (
            "post",
            "POST /seat HTTP/1.1\r\nHost: 127.0.0.1:9500\r\n\r\n",
        ),
        (
            "http-1.0",
            "GET /seat HTTP/1.0\r\nHost: 127.0.0.1:9500\r\n\r\n",
        ),
    ] {
        let (status, result) = reviewed(text, &policy);
        row(&mut out, &format!("{name}\t{status}\t{result}"));
    }

    check("handshake", "handshake.txt", &out);
}
