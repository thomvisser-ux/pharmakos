<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# CLAUDE.md — Pharmakos

The project's rules live in one file, so this one includes it rather than repeating it.

@AGENTS.md

Read that file before your first edit. What follows is only the Claude Code-specific part.

## Before you touch anything

1. **The toolchain is installed and `cargo xtask ci` is green.** rustc 1.98.1 (MSVC host), `protoc`
   36, `buf` 1.73, `cargo-deny`, `reuse` and Godot 4.7.2 are all present, so compile your work and
   run `cargo xtask ci --quick` as the inner loop and the full `cargo xtask ci` before you open a
   PR. Most crates are still empty placeholders, so a step whose input does not exist yet reports
   `skipped` with a reason — that is the harness working, not a failure to work around. The
   `PLACEHOLDER` rule is unchanged: mark every guessed value
   `// PLACEHOLDER: <what, who decides, when>` and list it in your PR.
2. **Check the design precedence before implementing a rule**: `docs/design/decisions-log.md` §2.7 >
   the spec (`docs/spec/pharmakos-spec-v0.6.html`) > `docs/design/co-design-gameplan-api.md`. See
   `docs/design/README.md`. The co-design doc predates the v1 simplification and still describes
   MCP tools, flags/branch/repeat, and a WASM planner — all of which are **not in v1**.
3. **Confirm nobody else owns your crate.** At most 2–3 agents run at once, each in its own git
   worktree, and two agents never edit the same crate (AGENTS.md §6).

## Stop and ask

Open a PR and stop — do not merge, do not work around it — when a task takes you into:

- a contract file: `proto/**`, `buf.*`, `clippy.toml`, workspace `[lints]`, `[profile.*]`,
  determinism code (state hash, RNG streams, fixed-point types, snapshot/replay format, tick loop,
  the `research` feature), `.github/workflows/**`, `xtask`'s definition of `ci`, licence files,
  or these harness docs (AGENTS.md §5);
- adding a dependency that is not on the approved list (AGENTS.md §3);
- anything on the "what not to build in v1" list (AGENTS.md §11) — script runtimes, MCP, an SDK, a
  published API, AI seats, manual control, playbook flags/branch/repeat;
- a `#[allow]` that would defeat a determinism lint.

Never disable a determinism lint to make a build pass, never regenerate golden hashes to make a red
test green without explaining the behaviour change that moved them, and never add a second hash
function or a shadow copy of a `.proto`.

## Permissions

Do not use bypass-permissions mode in this repository, and never against a real seat. The
contract-file rule depends on approval prompts actually happening; bypass mode removes the
mechanism rather than speeding it up.

**Owner's delegation for the walking skeleton (decisions-log §2.7 items 85 and 86, 2026-09-14).**
Two relaxations apply to the owner's own Claude Code session only, for the duration of the
walking skeleton, and are revoked by the owner's word at any time:

- The main assistant session may **merge** a skeleton PR, contract PRs included, once `cargo xtask
  ci` is green on the branch and the two adversarial reviews have been applied; the owner reviews
  the stage demo (AGENTS.md §10 item 8) rather than each PR. Every merge is still a PR whose body
  names the contract paths it touches. Sub-agents still open a PR and stop.
- The owner accepts **bypass-permissions mode** in that same session. The ban above stays in force
  for every sub-agent and for any session that touches a real seat.

## Commands

```sh
cargo fmt --all           # rustfmt defaults for edition 2024; the tree is formatted
cargo xtask ci            # the whole check suite — what CI runs, and it is green
cargo xtask ci --quick    # fmt, clippy, unit tests — the inner loop
cargo xtask ci --fix      # rustfmt plus machine-applicable clippy fixes
git commit -s             # DCO sign-off is mandatory on every commit
git worktree add ../pharmakos-<task> -b feat/<crate>-<task>
```

`cargo xtask ci` is the single entry point. If something is green locally and red in CI, that is an
`xtask` bug worth fixing, not a reason to run a different command. It is complete for harness part 1
(`xtask/src/main.rs`, twelve steps, covering AGENTS.md §9 items 1–9) and green — ten `ok`, two
`skipped` with reasons; a failure in it is a bug to fix rather than a reason to reach for another
command.

## When you finish

- Say which crates you touched, so the next agent can pick a disjoint set.
- List every `PLACEHOLDER` you left and who has to resolve it.
- Conventional-commit subject, `Signed-off-by:` as the last line, SPDX header on every new file
  (AGENTS.md §8).
- If a rule in AGENTS.md turned out to be wrong or missing, say so in the PR. Do not edit it
  yourself — it is a contract file.
