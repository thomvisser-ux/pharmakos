// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The security surface's negative tests, each named after the rule it enforces.
//!
//! Skeleton plan T9's acceptance line asks for exactly this, and asks for it
//! first: "negative tests as first-class citizens, each named after the rule it
//! enforces -- `binding_is_localhost_only`, `an_origin_mismatch_is_refused`,
//! `a_seat_token_can_never_hold_spectate_nofog`,
//! `admin_cannot_read_another_seats_draft`,
//! `a_fogged_seat_never_sees_an_unseen_voxel`".
//!
//! They live in an integration test rather than beside the code they check so
//! that the five are together, under the names the plan gave them, where the
//! next person to touch the gateway will find them. Several are also asserted
//! at the unit that enforces them; that is not duplication, it is the
//! difference between "the function refuses" and "the surface refuses".
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml sets allow-expect-in-tests and allow-unwrap-in-tests, but that configuration only recognises #[test] functions and #[cfg(test)] modules -- not an integration test's helper functions. A panic is this file's failure report (clippy.toml's own wording)."
)]

use pharmakos_gateway::error::Code;
use pharmakos_gateway::fog::{Audience, Blind, FogFilter, FogPolicy, KnownVoxels, Viewer};
use pharmakos_gateway::handshake::{self, Policy, Refusal};
use pharmakos_gateway::net::Listener;
use pharmakos_gateway::rpc;
use pharmakos_gateway::scopes::{Scope, ScopeSet};
use pharmakos_gateway::surface::{Draft, Surface};
use pharmakos_gateway::time::MatchTime;
use pharmakos_gateway::token::{Subject, Token};
use pharmakos_proto::gp::api::v1::status::Phase;
use pharmakos_proto::gp::v1::Voxel;
use pharmakos_proto::json::Json;
use pharmakos_sim::math::quantity::{Ms, Tick};
use pharmakos_sim::rules::RulesTable;
use pharmakos_sim::tables::SeatId;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

const MATCH: &str = "m-0001";
const SEED: u64 = 0x00ca_5cad_ed00_0001;

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
        SEED,
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

fn seat_token(surface: &mut Surface, seat: u8) -> Token {
    let (token, _) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(seat)),
            MATCH,
            ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::PlanSubmit, Scope::Docs]),
            Tick::ZERO,
        )
        .expect("minted");
    token
}

fn call(surface: &mut Surface, token: &Token, method: &str, params: &str) -> Json {
    let text = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
    let request = rpc::parse(&text).expect("well formed");
    surface.call(Some(token), &request, &Blind)
}

fn error_code(response: &Json) -> String {
    response
        .get("error")
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
        .map_or_else(
            || String::from("<no error>"),
            |value| match value {
                Json::String(text) => text.clone(),
                other => format!("{other:?}"),
            },
        )
}

// ---------------------------------------------------------------------------
// 1. The binding
// ---------------------------------------------------------------------------

/// Spec section 12 and AGENTS.md section 7: `127.0.0.1` and `::1`, and nothing
/// else, with no flag that widens it.
///
/// Two halves. The listener binds loopback and only loopback -- which the type
/// enforces, because [`Listener::bind`] takes a port and has nowhere to put an
/// address. And the `Host` check refuses a request that reached the loopback
/// port under somebody else's name, which is the half a bound socket cannot do
/// anything about: a name that resolves to `127.0.0.1` reaches this port
/// legitimately at the IP layer.
#[test]
fn binding_is_localhost_only() {
    let listener = Listener::bind(0).expect("an ephemeral loopback port");
    let port = listener.port();
    let addresses = listener.addresses();
    assert!(!addresses.is_empty(), "at least one loopback family binds");
    for address in &addresses {
        assert!(address.ip().is_loopback(), "{address} is not loopback");
        assert_eq!(address.port(), port, "one gateway, one port");
    }

    // The socket is reachable from loopback.
    let accepting = listener.accept();
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("loopback connects");
    client.write_all(b"GET /seat HTTP/1.1\r\n").expect("wrote");
    let connection = accepting.recv().expect("accepted");
    assert!(connection.peer.ip().is_loopback());

    // And a request that arrives here claiming another authority is refused,
    // whatever the IP layer thinks.
    let policy = Policy::loopback(port);
    let refusal = handshake::review(
        &upgrade_request(&format!("pharmakos.example:{port}"), None),
        &policy,
    )
    .expect_err("a foreign Host");
    assert!(matches!(refusal, Refusal::ForeignHost(_)), "{refusal:?}");
    assert_eq!(refusal.status(), 403);

    // A loopback authority at another port is refused too: that is somebody
    // else's service on this machine.
    let refusal = handshake::review(
        &upgrade_request(&format!("127.0.0.1:{}", port.wrapping_add(1)), None),
        &policy,
    )
    .expect_err("another port");
    assert!(matches!(refusal, Refusal::ForeignHost(_)), "{refusal:?}");

    for authority in [
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
        format!("localhost:{port}"),
    ] {
        handshake::review(&upgrade_request(&authority, None), &policy)
            .unwrap_or_else(|refusal| panic!("{authority} is loopback: {refusal:?}"));
    }
}

/// A conforming upgrade for `authority`, optionally carrying an `Origin`.
fn upgrade_request(authority: &str, origin: Option<&str>) -> String {
    let origin_line = origin.map_or_else(String::new, |value| format!("Origin: {value}\r\n"));
    let token = "0".repeat(63) + "1";
    format!(
        "GET /seat HTTP/1.1\r\n\
         Host: {authority}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {token}\r\n\
         {origin_line}\
         \r\n"
    )
}

// ---------------------------------------------------------------------------
// 2. The Origin check
// ---------------------------------------------------------------------------

/// Every v1 client is a native process and sends no `Origin`; a browser always
/// sends one; the allow-list is empty. So an `Origin` header at all is a refusal
/// with 403, and the absence of one is what a legitimate client looks like.
#[test]
fn an_origin_mismatch_is_refused() {
    let policy = Policy::loopback(9500);
    for origin in [
        "http://evil.example",
        "https://evil.example",
        "http://127.0.0.1:9500",
        "null",
    ] {
        let refusal = handshake::review(&upgrade_request("127.0.0.1:9500", Some(origin)), &policy)
            .expect_err("refused");
        assert!(
            matches!(refusal, Refusal::ForeignOrigin(_)),
            "{origin}: {refusal:?}"
        );
        assert_eq!(refusal.status(), 403);
        let response = handshake::refusal_response(&refusal);
        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden\r\n"),
            "{response}"
        );
        assert!(
            !response.contains(origin),
            "a refusal never echoes the value back: {response}"
        );
    }

    // No Origin at all: a native client, and the only shape v1 accepts.
    handshake::review(&upgrade_request("127.0.0.1:9500", None), &policy).expect("a native client");

    // And an allow-list entry, which is the mechanism the roadmap's web
    // spectator would use, works exactly and only for the listed string.
    let allowed = Policy {
        port: 9500,
        allowed_origins: vec![String::from("https://spectator.example")],
    };
    handshake::review(
        &upgrade_request("127.0.0.1:9500", Some("https://spectator.example")),
        &allowed,
    )
    .expect("the listed origin");
    assert!(
        handshake::review(
            &upgrade_request("127.0.0.1:9500", Some("https://spectator.example.evil")),
            &allowed,
        )
        .is_err(),
        "the match is exact, never a prefix"
    );
}

// ---------------------------------------------------------------------------
// 3. The token invariant
// ---------------------------------------------------------------------------

/// Spec section 12: "Seat tokens can never hold `spectate.nofog`; only separate
/// spectator tokens can."
///
/// Refused at the mint, so the token cannot exist -- and therefore cannot leak,
/// cannot be revoked-but-cached, and cannot be granted by a later code path that
/// forgot to check. The whole scope set is refused, not filtered down: a mint
/// that quietly dropped a scope would hand back a token the caller believed was
/// something else.
#[test]
fn a_seat_token_can_never_hold_spectate_nofog() {
    let mut surface = surface();
    for extra in [
        ScopeSet::of(&[Scope::SpectateNofog]),
        ScopeSet::of(&[Scope::Observe, Scope::SpectateNofog]),
        ScopeSet::of(&[Scope::Observe, Scope::Plan, Scope::SpectateNofog]),
    ] {
        let error = surface
            .tokens()
            .mint(Subject::Seat(SeatId::new(0)), MATCH, extra, Tick::ZERO)
            .expect_err("refused");
        assert_eq!(error.code, Code::ForbiddenScope);
        assert!(
            error.message.contains("spectate.nofog"),
            "{}",
            error.message
        );
    }
    assert!(
        surface.tokens().is_empty(),
        "a refused mint stores nothing at all"
    );

    // A spectator token may hold it, which is the other half of the sentence.
    surface
        .tokens()
        .mint(
            Subject::Spectator,
            MATCH,
            ScopeSet::of(&[Scope::Observe, Scope::SpectateNofog]),
            Tick::ZERO,
        )
        .expect("a spectator may");

    // And the scope gates no method, so nothing a seat can call is out of reach
    // because of it (decisions-log item 26: scopes gate spectator tokens only).
    let token = seat_token(&mut surface, 0);
    let response = call(&mut surface, &token, "get_segment_feed", "{}");
    assert!(response.get("result").is_some(), "{response:?}");
}

// ---------------------------------------------------------------------------
// 4. Admin and secrecy
// ---------------------------------------------------------------------------

/// Spec section 12: "Admin covers lobby and match control and can never read
/// another seat's playbooks, drafts or knowledge."
///
/// Three ways in, all refused: the store's own accessor, the JSON-RPC method,
/// and the scope an admin token would need to call it -- which it cannot even be
/// minted with, because a draft belongs to a seat and `admin` has none.
#[test]
fn admin_cannot_read_another_seats_draft() {
    let mut surface = surface();
    surface
        .store_draft(
            Subject::Seat(SeatId::new(0)),
            SeatId::new(0),
            Draft {
                draft_id: String::from("d1"),
                label: String::from("east push"),
                round: 1,
            },
        )
        .expect("a seat stores its own");

    // 1. The store.
    let error = surface
        .seat_state(Subject::Admin, SeatId::new(0))
        .expect_err("refused");
    assert_eq!(error.code, Code::ForbiddenScope);
    assert!(error.message.contains("drafts"), "{}", error.message);

    // 2. Another seat, by the same gate.
    let error = surface
        .seat_state(Subject::Seat(SeatId::new(1)), SeatId::new(0))
        .expect_err("refused");
    assert_eq!(error.code, Code::ForbiddenScope);

    // 3. A spectator, for completeness: `spectate.nofog` lifts fog, not secrecy.
    assert_eq!(
        surface
            .seat_state(Subject::Spectator, SeatId::new(0))
            .expect_err("refused")
            .code,
        Code::ForbiddenScope
    );

    // 4. The method. An admin token cannot even be minted with `plan`, so the
    //    call is refused before a handler could read anything.
    let error = surface
        .tokens()
        .mint(
            Subject::Admin,
            MATCH,
            ScopeSet::of(&[Scope::Admin, Scope::Plan]),
            Tick::ZERO,
        )
        .expect_err("refused");
    assert_eq!(error.code, Code::ForbiddenScope);

    let (admin, _) = surface
        .tokens()
        .mint(
            Subject::Admin,
            MATCH,
            ScopeSet::of(&[Scope::Admin, Scope::Observe]),
            Tick::ZERO,
        )
        .expect("an admin token without plan");
    let response = call(&mut surface, &admin, "list_drafts", "{}");
    assert_eq!(error_code(&response), "FORBIDDEN_SCOPE");

    // And the seat itself still reads its own.
    let owner = seat_token(&mut surface, 0);
    let response = call(&mut surface, &owner, "list_drafts", "{}");
    let drafts = response
        .get("result")
        .and_then(|result| result.get("drafts"))
        .cloned()
        .expect("a listing");
    let Json::Array(entries) = drafts else {
        panic!("drafts is an array");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries.first().and_then(|entry| entry.get("draft_id")),
        Some(&Json::String(String::from("d1")))
    );
}

// ---------------------------------------------------------------------------
// 5. Fog
// ---------------------------------------------------------------------------

/// Spec section 12 and decisions-log item 26: fogged by default, and a seat sees
/// what a seat has seen.
///
/// Through the whole surface, not just the filter: the event is on the bus, the
/// feed is asked for by a real call, and the voxel is not in the seat's vision.
#[test]
fn a_fogged_seat_never_sees_an_unseen_voxel() {
    let mut surface = surface();
    let secret = Voxel {
        x: 300,
        y: 300,
        z: 40,
    };
    surface
        .publish(pharmakos_gateway::feed::Event {
            at_ms: Ms::new(1_000),
            kind: pharmakos_gateway::feed::Kind::new("beacon_placed").expect("a kind"),
            text: String::from("Seat 1 placed a beacon."),
            audience: Audience::World {
                owner: Some(SeatId::new(1)),
                at: secret,
            },
        })
        .expect("published");

    let token = seat_token(&mut surface, 0);
    let response = call(&mut surface, &token, "get_segment_feed", "{}");
    let events = response
        .get("result")
        .and_then(|result| result.get("events"))
        .cloned()
        .expect("a feed");
    assert_eq!(
        events,
        Json::Array(Vec::new()),
        "seat 0 has seen nothing, so it is told nothing"
    );

    // The filter itself, over every combination that matters.
    let policy = FogPolicy::fogged();
    let blind = FogFilter::new(&policy, &Blind);
    let elsewhere = Audience::World {
        owner: Some(SeatId::new(1)),
        at: secret,
    };
    assert!(!blind.visible(Viewer::Seat(SeatId::new(0)), &elsewhere));
    assert!(!blind.visible(Viewer::Spectator { nofog: false }, &elsewhere));
    assert!(!blind.visible(Viewer::Admin, &elsewhere));
    assert!(
        blind.visible(Viewer::Seat(SeatId::new(1)), &elsewhere),
        "its own"
    );
    assert!(blind.visible(Viewer::Spectator { nofog: true }, &elsewhere));

    // Having seen it once is what changes the answer -- nothing else.
    let mut vision = KnownVoxels::new();
    vision.see(SeatId::new(0), &secret);
    let seen = FogFilter::new(&policy, &vision);
    assert!(seen.visible(Viewer::Seat(SeatId::new(0)), &elsewhere));
    assert!(
        !seen.visible(
            Viewer::Seat(SeatId::new(0)),
            &Audience::World {
                owner: Some(SeatId::new(1)),
                at: Voxel {
                    x: 301,
                    y: 300,
                    z: 40
                },
            }
        ),
        "one voxel over is a different voxel"
    );
}

// ---------------------------------------------------------------------------
// The surface's own shape
// ---------------------------------------------------------------------------

/// A revoked token stops working at the next call, without the match pausing or
/// any token being reissued.
#[test]
fn a_token_revoked_from_the_lobby_stops_working() {
    let mut surface = surface();
    let (token, handle) = surface
        .tokens()
        .mint(
            Subject::Seat(SeatId::new(0)),
            MATCH,
            ScopeSet::of(&[Scope::Observe]),
            Tick::ZERO,
        )
        .expect("minted");
    assert!(
        call(&mut surface, &token, "get_status", "{}")
            .get("result")
            .is_some()
    );
    assert!(surface.tokens().revoke(handle));
    let response = call(&mut surface, &token, "get_status", "{}");
    assert_eq!(error_code(&response), "UNAUTHENTICATED");
}

/// Elimination and match end change the *policy*, and the seat keeps the token
/// it already had (spec section 12: no token is reissued mid-match).
#[test]
fn elimination_lifts_fog_without_reissuing_a_token() {
    let mut surface = surface();
    let token = seat_token(&mut surface, 0);
    surface
        .publish(pharmakos_gateway::feed::Event {
            at_ms: Ms::new(1_000),
            kind: pharmakos_gateway::feed::Kind::new("ore_delivered").expect("a kind"),
            text: String::from("Seat 1 delivered ore."),
            audience: Audience::World {
                owner: Some(SeatId::new(1)),
                at: Voxel { x: 9, y: 9, z: 9 },
            },
        })
        .expect("published");

    let before = call(&mut surface, &token, "get_segment_feed", "{}");
    let events = before
        .get("result")
        .and_then(|result| result.get("events"))
        .cloned();
    assert_eq!(events, Some(Json::Array(Vec::new())));

    surface.fog().eliminate(SeatId::new(0));
    let after = call(&mut surface, &token, "get_segment_feed", "{}");
    let Some(Json::Array(events)) = after
        .get("result")
        .and_then(|result| result.get("events"))
        .cloned()
    else {
        panic!("a feed");
    };
    assert_eq!(events.len(), 1, "the same token now sees the world");
}

/// The private cache is enforcement and not cryptography, and the folder says
/// so before anybody zips it up.
#[test]
fn the_private_match_cache_says_what_it_is() {
    let root = std::env::temp_dir()
        .join("pharmakos-gateway-tests")
        .join("security-cache");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("scratch");
    let cache = pharmakos_gateway::cache::MatchCache::open(&root, MATCH, SEED).expect("opened");
    let mut readme = String::new();
    std::fs::File::open(cache.directory().join("README.txt"))
        .expect("README.txt")
        .read_to_string(&mut readme)
        .expect("read");
    assert!(readme.contains("NOT ENCRYPTED"), "{readme}");
    assert!(
        readme.contains("cryptography"),
        "the folder does not claim more than it is"
    );
}
