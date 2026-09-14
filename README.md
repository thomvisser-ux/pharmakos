<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Pharmakos: The Sealed Order

*The cure and the poison are the same thing. Nothing arrives over the air.*

Pharmakos is an open-source voxel strategy game about orders you cannot take back.
Each seat seals a **playbook** during the planning **Lull**, then watches the **Push**
play out like a movie: nobody has live control, and a commander must walk to a beacon
and **interface** on site to change anything. The world is destructible voxels; the
beacon is the unit of power; commander time is the real currency.

The v1 rule is: **everything that executes is Rust — the commander's built-in operator,
the mandates on beacons, the programs in units and buildings — and everything the player
touches is data.** Playbooks and templates are JSONC files, edited in the in-game editor
or by hand. Opponents are the built-in operator. External scripting (v1.1) and live AI
seats (v1.2) are roadmap, reserved now only as proto seams.

**Status: pre-spike.** No toolchain is installed and **nothing here has ever been compiled,
formatted or executed.** This repository holds the specification, the design record, the
workspace skeleton below, and a first cut of harness part 1 written ahead of the toolchain:
`AGENTS.md` / `CLAUDE.md`, `cargo xtask ci` (`xtask/src/main.rs`), the lint configuration
(`clippy.toml`, `deny.toml`, `[workspace.lints]`), the CI workflows and the golden-file and
determinism conventions. Treat the first `cargo fmt --all` and the first `cargo xtask ci` as
a shakedown. The next steps are the stack spikes (G4 cross-OS determinism with save/restore
and fork equivalence, G1 mesher and remesh in Godot 4.7), then the walking skeleton.

## Repository layout

```
crates/sim/            deterministic simulation: 20 Hz fixed tick, integer maths, SoA tables,
                       32³ copy-on-write chunks, runner, mandates, programs, Quartermaster,
                       power grid, snapshot/restore, replay, per-tick xxh3 state hash
crates/plan-core/      verify · estimate · render · patch · canonicalise (in process with the gateway)
crates/verifier/       seal inspection: QUICK and FULL pipelines, diagnostics, report_hash
crates/operator/       the built-in operator: generate, file the safe playbook, execute
crates/gateway/        Seat Gateway: tokens · scopes · fog filter · rate limits · event bus · snapshots
crates/gamectl/        CLI: verify, schema, docs, scenarios, seat doctor
crates/client-gdext/   thin gdext crate; the Godot 4.7 client's GDScript side is views and editor only
crates/proto/          gp.v1 and gp.api.v1 generated types — the single schema source
xtask/                 cargo xtask ci: the one command CI and contributors run
proto/                 gp.v1 and gp.api.v1 .proto files, buf config — contract files
examples/playbooks/    example playbooks (JSONC), MIT OR Apache-2.0
assets/                art and audio, CC BY-SA 4.0
docs/spec/             the design specification (current: v0.6)
docs/design/           decisions log, handoffs, investigations — see docs/README.md
```

Workspace-wide build settings live in `Cargo.toml` (`[workspace.lints]`, `[profile.release]`),
`.cargo/config.toml` and `rust-toolchain.toml`; dependency and licence policy lives in `deny.toml`.

## Documentation

- **Specification:** [`docs/spec/pharmakos-spec-v0.6.html`](docs/spec/pharmakos-spec-v0.6.html)
  — sections 1–19; section 15 is the architecture this layout follows, section 17 the build plan.
- **Design record:** [`docs/design/decisions-log.md`](docs/design/decisions-log.md) — every decision
  with its reasoning, including the draft-6 review items. Start at [`docs/README.md`](docs/README.md).

Precedence when two documents disagree, highest first: **`docs/design/decisions-log.md` §2.7
> the spec (v0.6) > `docs/design/co-design-gameplan-api.md`.** §2.7 is the draft-6 review
record and overrides everything written earlier; the co-design doc predates the v1
simplification. See [`docs/design/README.md`](docs/design/README.md), which is the
authoritative statement of this order. When code and the documents disagree, that is a bug in
one of them and the decisions log usually says which.

## Engineering constraints that are not negotiable

These are contract-level (spec §15). Changes to `.proto` files, lints and determinism
code need owner approval.

- **Determinism.** Fixed 20 Hz tick, integer maths (position Q16.16, squared distance
  Q32.32, u16 angles, integer HP and $), split seeded RNG streams, ordered collections,
  per-tick xxh3 state hash, cross-OS hash equality in CI. Overflow checks stay on in
  every profile.
- **Lints forbid** float arithmetic, `as` casts, `HashMap`/`HashSet` and wall-clock time
  outside the walled presentation/solve module.
- **No dry runs.** `fork` exists only behind the compile-time `research` feature, which
  only `crates/sim` may define and release builds never enable; CI builds both
  configurations. A CI dependency check bars `plan-core`, `verifier`, `operator` and
  `gateway` from reaching it.
- **Schema.** Protobuf is the single source (`gp.v1`, `gp.api.v1`); playbooks live on disk
  as JSONC (canonical proto JSON plus comments); `buf breaking` runs in CI.

## Security constraints

- The Seat Gateway binds **127.0.0.1 / ::1 only**, with Host and Origin checks.
- Per-seat 256-bit tokens tied to match and seat, revocable from the lobby. Scopes:
  `observe`, `plan`, `plan.submit`, `docs`, `spectate.nofog`, `admin`. A seat token can
  never hold `spectate.nofog`; `admin` can never read another seat's playbooks, drafts or
  knowledge.
- Fog is a server-side per-match policy applied by the gateway's fog filter. Playbooks,
  drafts, the notebook and the private replay cache never leave the gateway for another seat.
- **No filesystem or network access through playbooks** — they are data, not scripts, and
  nothing user-written executes in v1.
- **No network telemetry.** Playtest bundles are written to a local file the tester sends
  manually, and exclude playbooks, drafts, the notebook and the private replay.
- **The game never holds keys.**

## Licences

- Game code (sim, client, operator, gateway, gamectl): **GPL-3.0-or-later**.
- Schemas (`.proto` and generated JSON Schema), docs, `llms.txt`, example playbooks:
  **MIT OR Apache-2.0**.
- Art and audio: **CC BY-SA 4.0** (one-way compatible into GPLv3).

A REUSE-style licence manifest, per-directory `LICENSE` files and SPDX headers cover the
tree; contributions require a DCO sign-off (`git commit -s`).
