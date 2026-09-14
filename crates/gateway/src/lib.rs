// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Seat Gateway. Role from spec §12, gateway layer of spec §15 (Architecture).
//!
//! One internal API with several clients: the Godot editor, the `gamectl` CLI and the
//! built-in operator all call it over JSON-RPC on a localhost WebSocket. The editor is
//! simply one more client — it runs no validation or time maths of its own. Nothing is
//! published in v1: no MCP server, no SDK, no script runtime. The gateway is nonetheless
//! kept agent-shaped, because publishing it later should be a documentation and hardening
//! job rather than a rewrite.
//!
//! # What lives here
//!
//! Tokens, scopes, the fog filter, rate limits, the event bus and snapshots. Method
//! groups: status, memory (the private seat notebook), knowledge, docs, planning, commit,
//! and the during-play segment feed. Plan core runs in process behind it.
//!
//! # Security (spec §12) — contract-level, not implementation detail
//!
//! * Binds **`127.0.0.1` / `::1` only**, with Host and Origin checks.
//! * Per-seat 256-bit tokens tied to the match and seat, revocable from the lobby.
//!   Scopes: `observe`, `plan`, `plan.submit`, `docs`, `spectate.nofog`, `admin`.
//! * A seat token can **never** hold `spectate.nofog`; only a separate spectator token
//!   can. `admin` covers lobby and match control and can never read another seat's
//!   playbooks, drafts or knowledge.
//! * Fog is a per-match policy applied server-side by the fog filter: fogged by default,
//!   no-fog in casual matches, unlocked on elimination and at match end. No token is
//!   reissued mid-match. Live standings carry a seat's own score and rank only.
//! * Secrecy: playbooks, drafts, the notebook and the private replay cache never leave the
//!   gateway for another seat.
//! * Rate limits and an audit log. **No filesystem or network access through playbooks** —
//!   they are data, not scripts. No network telemetry. The game never holds keys.
//! * Errors are a closed set of codes (`PHASE_CLOSED`, `STALE_SNAPSHOT`,
//!   `NO_QUALIFYING_PLAN`, `RATE_LIMITED`, …). An invalid playbook is not a method error:
//!   it returns a full report with `qualifies: false`.
//!
//! # Rules this crate is held to
//!
//! * **No `research` feature.** Only `crates/sim` defines it; the gateway may never enable
//!   or transitively reach it, so no seat can reach `fork` and "no dry runs" holds.
//!   `cargo xtask ci` enforces this.
//! * Deterministic behaviour on deterministic inputs: read results are structured JSON
//!   plus deterministic template prose, pagination uses opaque cursors tied to the
//!   snapshot, and every result carries a `_status` footer with the phase and timer.
//!
//! Nothing is implemented yet — placeholder until the walking skeleton, where gateway auth
//! and scopes are built to full quality as a never-churn contract.
