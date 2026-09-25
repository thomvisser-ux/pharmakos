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
//! # What this crate is
//!
//! **The security surface was built once and before any method hung off it** (skeleton plan
//! T9). That sequencing is the point: the surface is what the whole roadmap inherits, and
//! a surface grown outwards from a method slice is the most expensive avoidable rework in
//! the stage (plan section 8, risk R8). So the transport, the tokens, the scope check, the
//! fog policy, the rate limiter, the audit log, the event feed and the private match cache
//! were finished first, and the gameplay methods came afterwards, onto a surface that did
//! not move to receive them.
//!
//! **T13 hung them there, and put a match behind them.** The gateway now hosts the match
//! (AGENTS.md section 3's crate map, spec section 15): [`host`] owns the runner and is the
//! **only** place in this crate that drives it, [`surface`] serves every method the
//! skeleton's clients call, and `tests/confinement.rs` asserts over this crate's own source
//! text that no handler can reach a stepping call -- which is what "no dry runs" means for
//! a crate that has a world in the same process (AGENTS.md section 3 rule 2).
//!
//! **T13b gave the match its orders.** T13 was built against a sim that had only the
//! interpreter seam, so a verified submission was sealed in the seat's own private store
//! and the hosted world never heard of it: every commander stood still for the whole Push
//! (decisions-log item 103 (1)). `submit_plan` now compiles the playbook at the door --
//! the sim's `Plan::compile` is a second question from the verifier's, "can this build
//! execute it" rather than "may this be sealed", and a refusal is a method error naming
//! the construct -- and [`surface::Surface::begin_push`] seals every seat's compiled plan
//! into the runner before the Lull ends.
//!
//! | Module | What it is |
//! |---|---|
//! | [`sha1`] | SHA-1, for `Sec-WebSocket-Accept` and nothing else |
//! | [`host`] | The match host: the one place this crate seals into or steps a `Runner` |
//! | [`routes`] | plan-core's `TravelEstimator`, over the sim's estimator (item 100 (1)) |
//! | [`schema`] | `get_schema`, generated from the checked-in descriptor set |
//! | [`strings`] | This crate's section of the one English string table |
//! | [`view`] | The world's types as the wire's: voxels, beacon ids, mandates |
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
//! | [`viewfeed`] | The view's derived, unhashed state: the generated map, the stamps, the entitled bytes |
//! | [`serve`] | The host loop: one config line in, one announce line out, one surface behind many connections |
//! | [`advice`] | What the built-in operator advises a seat it does not play: the safe playbook and the wizard's suggestions |
//! | [`cache`] | The private match cache: where it goes, and what is in it |
//! | [`save`] | The save container and the private replay's inputs: what a Lull boundary writes, and what a resume reads back |
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
//!   error: it returns a full report with `qualifies: false`, which
//!   `tests/methods.rs` asserts on three shapes of wrong.
//!
//! # Rules this crate is held to
//!
//! * **No `research` feature.** Only `crates/sim` defines it; the gateway may never enable
//!   or transitively reach it, so no seat can reach `fork` and "no dry runs" holds.
//!   `cargo xtask ci` enforces this.
//! * **No dry runs.** The gateway hosts the match, so it does drive one -- in [`host`], in
//!   four calls, and nowhere else. No method handler may: `verify_plan`, `estimate_route`,
//!   `render_plan`, `patch_plan`, `get_economy_forecast` and `instantiate_template` all
//!   answer from the frozen snapshot and the rules table, and `estimate_route` goes through
//!   [`routes::RouteAdapter`], which owns its own search graph and never has a `World` in
//!   its hand at all. `submit_plan` **compiles** a playbook, which is a pure function of
//!   the playbook and the rules table and steps nothing; sealing the result into the match
//!   is the host's call and not a handler's.
//! * **No clock.** The gateway is not a walled crate (AGENTS.md section 4.5), so it reads
//!   no wall clock at all: time enters as the host's tick through [`time::MatchTime`], the
//!   rate limiter counts per tick and per window of ticks, and the audit log stamps a tick
//!   and a sequence number. A Lull consumes no sim tick, so the gateway's own tick counts
//!   it from two numbers it is *given* -- the Lull's length from the rules table and how
//!   much of it the client says is left ([`surface::Surface::set_phase_remaining_ms`]) --
//!   which is host time given to the gateway rather than a clock it read. A
//!   [`std::time::Duration`] on a socket timeout is a socket option rather than a clock
//!   read, and is the one place a duration appears.
//! * Deterministic behaviour on deterministic inputs: read results are structured JSON
//!   plus deterministic template prose, pagination uses opaque cursors tied to the
//!   snapshot, and every result carries a `_status` footer with the phase and timer.

pub mod advice;
pub mod audit;
pub mod cache;
pub mod detail;
pub mod error;
pub mod feed;
pub mod fog;
pub mod frame;
pub mod handshake;
pub mod host;
pub mod limit;
pub mod net;
pub mod routes;
pub mod rpc;
pub mod save;
pub mod schema;
pub mod scopes;
pub mod serve;
pub mod session;
pub mod sha1;
pub mod strings;
pub mod surface;
pub mod time;
pub mod token;
pub mod view;
pub mod viewfeed;

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
