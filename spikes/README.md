<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# `spikes/` — throwaway code, deleted after week 5

This directory holds the four stack spikes of weeks 1–3 (spec section 17, row 1). **Nothing in here ships, and nothing in here is merged into the product.** The plans live in [`docs/spikes/`](../docs/spikes/README.md); this is where their toy builds go.

| Directory | Gate | Plan |
|---|---|---|
| `g4-determinism/` | G4 — cross-OS hashes, save/restore, fork equivalence | [G4-determinism.md](../docs/spikes/G4-determinism.md) |
| `g1-remesh/` | G1 — greedy mesher and remesh in Godot 4.7 | [G1-destruction-remesh.md](../docs/spikes/G1-destruction-remesh.md) |
| `g2-pathing/` | G2 + P3 — HPA\* repath and travel estimates | [G2-P3-pathing.md](../docs/spikes/G2-P3-pathing.md) |
| `g3-tick-budget/` | G3′ — synthetic tick check and the power budget | [G3prime-segment-budget.md](../docs/spikes/G3prime-segment-budget.md) |

## The rules

1. **Out of the workspace.** Each spike is its own standalone cargo workspace: every `spikes/*/Cargo.toml` carries an empty `[workspace]` table, which detaches it from any parent workspace. The product's root `Cargo.toml` also lists `exclude = ["spikes/*"]` — belt and braces, so a glob-added member or a stray manifest cannot pull spike code into the product build. Consequences: `cargo build` / `cargo test` at the repo root never touch this directory, `cargo xtask ci` skips it, and the determinism lints, the `buf breaking` check, coverage and the dependency check do not apply here. That is deliberate — spike code is allowed to be ugly, and the contract layer must not inherit its shortcuts.
2. **Nothing here is copied into `crates/`.** When the skeleton needs the same algorithm it is written again, against the real types, with the spike open for reference. Copying between spikes is fine (G3′ starts from G4's toy sim); copying out of `spikes/` is not.
3. **The findings live in `docs/`, not here.** Numbers go into the Results section of the relevant plan; each spike's one decision goes into `docs/design/decisions-log.md` §2.7. If a finding exists only as a comment in a file under `spikes/`, it will be lost when this directory is deleted — which is the point of the rule.
4. **Lifecycle.** Frozen at the `spike-end` tag at the end of week 3; **deleted from `main`** once harness part 1 is green (week 5). The tag keeps the code readable forever without keeping it in the tree.
5. **Licence hygiene anyway.** Source files carry an SPDX header — `GPL-3.0-or-later` for spike code that prototypes game crates, matching what the real crates will use — so the REUSE-style manifest stays green while this directory exists. Commits are DCO signed off (`git commit -s`) like every other commit.
6. **Never a dependency.** No product crate, workflow, script or document outside `docs/spikes/` may reference a path under `spikes/`. The spike workflows (`.github/workflows/spike-*.yml`) are deleted with the directory; what survives of them is re-expressed inside `cargo xtask ci`.

## Build outputs are git-ignored

`spikes/.gitignore` covers the build and measurement artefacts:

- `target/` — cargo output, per spike (gdext cdylibs in `g1-remesh/target/` are large);
- `.godot/`, `*.import`, `*.translation` — Godot's per-project import cache, regenerated on open;
- `*.csv`, `*.json`, `*.txt` traces, `*.png` screenshots — measurement artefacts. **Read them, record the summary in the plan's Results section, and let them be deleted.** The raw trace of a 9,600-tick run is not something to commit; the ten hashes it reduces to are.

The one exception worth making by hand: if a cross-OS hash mismatch is found, commit the *minimal* reproducing trace excerpt alongside the finding, force-added, so the diagnosis survives the directory.

## Running a spike

Each spike's plan file has a *Prerequisites* step. In summary, as of this writing nothing is installed on the build machine: **no `cargo`, no `protoc`/`buf`, no Godot**. Day 0 of week 1 installs the Rust toolchain (the **MSVC** ABI host on Windows, `stable-x86_64-pc-windows-msvc`, to match both CI's `windows-latest` and Godot's own ABI) and Godot 4.7. Every spike pins its toolchain in `spikes/<name>/rust-toolchain.toml`: "identical across operating systems" is only a meaningful claim at one pinned compiler version.

Nothing in this directory can be compiled before that, so every file here is written to be right once the toolchain exists. Version numbers and thresholds that cannot be known yet are marked `PLACEHOLDER` in the plans and must be replaced with a measurement, never with a guess.
