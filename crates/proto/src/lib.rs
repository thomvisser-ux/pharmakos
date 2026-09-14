// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generated Protobuf types. Role from spec §15 (Architecture), "Schema": **Protobuf is
//! the single source** — packages `gp.v1` (the game types) and `gp.api.v1` (the Seat
//! Gateway surface).
//!
//! Licensed MIT OR Apache-2.0, unlike the rest of the workspace, so the schema stays
//! reusable outside the GPL game.
//!
//! # How the schema is handled
//!
//! * Playbooks and templates live on disk as **JSONC**: canonical proto JSON plus
//!   comments, which the editor round-trips byte-for-byte.
//! * An in-house descriptor generator produces JSON Schema; Buf's plugin is the fallback.
//! * `buf breaking` runs in CI from day one. `.proto` files are contract files: changes
//!   need owner approval.
//!
//! # Reserved seams (spec §15, §18)
//!
//! Oneofs with only the `builtin` arm implemented — `operator {builtin|script}`,
//! `mandate {builtin|script}`, `program {builtin|script}` — plus `author_kind SCRIPT` and
//! a plan fingerprint. Alongside them, reserved field numbers: the v1 vocabulary's
//! absentees (`set_flag`, `clear_flag`, `branch`, `repeat`, the flag predicates), player
//! Dispatches, and field 7 on `Playbook` for the `kind` half of the envelope's
//! kind/version tag. `Playbook.schema_version` is the version half and exists today; the
//! `kind` half is a held field number, not a field, until the walking skeleton settles
//! what it distinguishes (spec §15, §17 — see the comment on the reservation in
//! `proto/gp/v1/playbook.proto`). Nothing on the script side executes in v1; the script
//! arm is v1.1 and the live conduit v1.2.
//!
//! Nothing is generated yet — placeholder until the schema work in the walking skeleton.
