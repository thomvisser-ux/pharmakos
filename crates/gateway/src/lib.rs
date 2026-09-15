// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Seat Gateway. Role from spec section 12, gateway layer of spec section 15
//! (Architecture).
//!
//! One internal API with several clients: the Godot editor, the `gamectl` CLI and the
//! built-in operator all call it over JSON-RPC on a localhost WebSocket. The editor is
//! simply one more client -- it runs no validation or time maths of its own. Nothing is
//! published in v1: no MCP server, no SDK, no script runtime. The gateway is nonetheless
//! kept agent-shaped, because publishing it later should be a documentation and hardening
//! job rather than a rewrite.
//!
//! # What this crate is, at T9
//!
//! **The security surface, built once and before any method hangs off it** (skeleton plan
//! T9). That sequencing is the point: the surface is what the whole roadmap inherits, and
//! a surface grown outwards from a method slice is the most expensive avoidable rework in
//! the stage (plan section 8, risk R8). So the transport, the tokens, the scope check, the
//! fog policy, the rate limiter, the audit log, the event feed and the private match cache
//! are all here and all tested, and the gameplay methods are not: [`surface`] answers
//! exactly the handful of methods the surface itself needs to be exercised end to end, and
//! every other method of the schema answers "T13 fills this".
//!
//! | Module | What it is |
//! |---|---|
//! | [`sha1`] | SHA-1, for `Sec-WebSocket-Accept` and nothing else |
//! | [`base64`] | Standard base64, for the same handshake |
//! | [`handshake`] | The RFC 6455 upgrade, with the `Host` and `Origin` checks |
//! | [`frame`] | RFC 6455 framing: masked client frames, text/close/ping/pong |
//! | [`rpc`] | JSON-RPC 2.0 requests and responses over [`pharmakos_proto::json`] |
//! | [`error`] | The closed error set, as `gp.api.v1.GatewayError.Code` |
//! | [`scopes`] | The method/scope table, read from T1's annotation |
//! | [`token`] | 256-bit per-seat tokens, minted, compared in constant time, revoked |
//! | [`limit`] | The rate limiter, counted in ticks and never in wall time |
//! | [`audit`] | The audit log: tick, sequence number, subject, method, outcome |
//! | [`fog`] | The per-match server-side fog policy and its filter |
//! | [`feed`] | The event bus, the 60 s digest cadence and the opaque cursors |
//! | [`cache`] | The private match cache: where it goes, and what is in it |
//! | [`detail`] | The read-method detail budgets, in the fixed salience order |
//! | [`time`] | The host's tick reaching the gateway, because the gateway has no clock |
//! | [`surface`] | Authentication, dispatch, the `_status` footer, the seat's private store |
//! | [`net`] | The listener, bound to `127.0.0.1` and `::1` and nothing else |
//! | [`session`] | One connection's life, over any `Read + Write` |
//!
//! # Security (spec section 12) -- contract-level, not implementation detail
//!
//! * Binds **`127.0.0.1` / `::1` only**, with Host and Origin checks ([`net`],
//!   [`handshake`]). There is no flag that widens it.
//! * Per-seat 256-bit tokens tied to the match and seat, revocable from the lobby
//!   ([`token`]). Scopes: `observe`, `plan`, `plan.submit`, `docs`, `spectate.nofog`,
//!   `admin`.
//! * A seat token can **never** hold `spectate.nofog`; only a separate spectator token
//!   can. `admin` covers lobby and match control and can never read another seat's
//!   playbooks, drafts or knowledge. Both invariants are asserted in
//!   [`token::TokenStore::mint`] and [`surface::Surface`], because neither is expressible
//!   in the schema.
//! * Fog is a per-match policy applied server-side by [`fog::FogFilter`]: fogged by
//!   default, no-fog in casual matches, unlocked on elimination and at match end. No token
//!   is reissued mid-match. Live standings carry a seat's own score and rank only.
//! * Secrecy: playbooks, drafts, the notebook and the private replay cache never leave the
//!   gateway for another seat. On a local host this is enforcement, not cryptography, and
//!   [`cache`] says so out loud rather than claiming more.
//! * Rate limits and an audit log ([`limit`], [`audit`]) are part of the feature, not a
//!   later hardening task. **No filesystem or network access through playbooks** -- they
//!   are data with a closed vocabulary, and `tests/confinement.rs` asserts that the
//!   vocabulary contains no path, no URL and no socket.
//! * Errors are the closed set of [`error::Code`]. An invalid playbook is not a method
//!   error: it returns a full report with `qualifies: false` (T13).
//!
//! # Rules this crate is held to
//!
//! * **No `research` feature.** Only `crates/sim` defines it; the gateway may never enable
//!   or transitively reach it, so no seat can reach `fork` and "no dry runs" holds.
//!   `cargo xtask ci` enforces this.
//! * **No clock.** The gateway is not a walled crate (AGENTS.md section 4.5), so it reads
//!   no wall clock at all: time enters as the host's tick through [`time::MatchTime`], the
//!   rate limiter counts per tick and per window of ticks, and the audit log stamps a tick
//!   and a sequence number. A [`std::time::Duration`] on a socket timeout is a socket
//!   option rather than a clock read, and is the one place a duration appears.
//! * Deterministic behaviour on deterministic inputs: read results are structured JSON
//!   plus deterministic template prose, pagination uses opaque cursors tied to the
//!   snapshot, and every result carries a `_status` footer with the phase and timer.

pub mod audit;
pub mod base64;
pub mod cache;
pub mod detail;
pub mod error;
pub mod feed;
pub mod fog;
pub mod frame;
pub mod handshake;
pub mod limit;
pub mod net;
pub mod rpc;
pub mod scopes;
pub mod session;
pub mod sha1;
pub mod surface;
pub mod time;
pub mod token;

pub use error::{Code, Error};
pub use surface::Surface;
pub use time::MatchTime;
pub use token::{Subject, Token, TokenStore};

/// The gateway's own version, stamped into the audit log's header so a log read
/// months later says which surface wrote it.
///
/// Bumped by the task that changes the surface's observable behaviour, not by
/// every edit.
pub const GATEWAY_VERSION: u32 = 1;
