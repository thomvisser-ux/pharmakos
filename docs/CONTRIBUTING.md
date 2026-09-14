<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Contributing to Pharmakos

The working rules — determinism, crate boundaries, contract files, security — are in
[`AGENTS.md`](../AGENTS.md) at the repository root. Read that first. This page covers only the legal
and procedural side: sign-off, licences, and how a change gets reviewed.

## Developer Certificate of Origin

Every commit needs a DCO sign-off. We use the [DCO 1.1](https://developercertificate.org/) verbatim:
by signing off you certify that you wrote the change, or have the right to submit it under the file's
licence.

```sh
git commit -s -m "feat(sim): add brownout ordering"
```

That adds the trailer, which must be the last line of the message and must be a real identity:

```
Signed-off-by: Ada Lovelace <ada@example.org>
```

The `dco` job in `.github/workflows/ci.yml` walks every commit in the pull request and fails if one
is missing the trailer. No sign-off, no merge. There is no CLA. If a branch is already written, fix
it in one go with `git rebase --signoff <base>`.

## Licences

Pharmakos is multi-licensed by directory, REUSE-style: every file carries an SPDX header, every
directory carries a `LICENSE`, and a manifest lists the exceptions.

| What | Licence | SPDX identifier |
|---|---|---|
| Game code — sim, client, operator, gateway, gamectl, xtask | GNU GPL v3 or later | `GPL-3.0-or-later` |
| Schemas (`.proto`, generated JSON Schema) **and the `crates/proto` crate that holds the generated types**, docs, `llms.txt`, example playbooks | MIT or Apache-2.0, at your option | `MIT OR Apache-2.0` |
| Art and audio assets | Creative Commons BY-SA 4.0 | `CC-BY-SA-4.0` |

`crates/proto` is the one permissive crate under `crates/`: it is the same public surface as the
`.proto` files, so a third-party tool must not have to take the GPL to use it. `REUSE.toml` carves
it out of `crates/**` by a later annotation, and `crates/proto/Cargo.toml` says so too.
`docs/DCO.txt` is outside every tier — it is the Linux Foundation's document and keeps its own
verbatim-copy terms.

Header every new file:

```rust
// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

Rules that follow from the split:

- **Schemas and example playbooks stay permissive** so anyone can write a tool against them. Do not
  copy GPL code into `proto/` or the example library.
- **CC BY-SA 4.0 is one-way compatible into GPLv3**, so assets may be used by the game but GPL code
  may not be pasted into an asset pipeline file and relicensed.
- **Third-party assets** keep their own SPDX line, their upstream attribution and a credits-screen
  entry. Audio comes from CC0 or CC-BY sources only, checked per track. Never add an asset whose
  licence you have not read.
- **Never commit keys, tokens or signing material** of any kind (`AGENTS.md` §7).

Console ports are out of scope: GPL and NDA'd console SDKs do not mix.

## Review flow

1. **Pick up a task and check it is free.** At most 2–3 contributors or agents work in parallel, and
   two people never edit the same crate at once.
2. **Work in a branch, in your own worktree.**
   `git worktree add ../pharmakos-<task> -b feat/<crate>-<task>`
3. **Keep the change inside one crate where you can.** Cross-crate changes need the owner to
   sequence them.
4. **Green `cargo xtask ci` before you open the PR** — the same command CI runs. Explain any golden
   file or determinism hash that moved, and why.
5. **Contract files stop at the PR.** Touching `proto/**`, lint configuration, determinism code, CI
   or licence files means: open the PR, describe the contract change, and wait for the owner. Do not
   merge and do not route around it (`AGENTS.md` §5).
6. **Commit messages** are conventional commits with a crate scope — `fix(verifier): reject unknown
   fields on load` — signed off, with a `Co-Authored-By:` trailer above the sign-off if an agent
   wrote it.
7. **The owner reviews and merges.** Review capacity is the project's bottleneck, so a small,
   self-contained, well-explained PR lands faster than a large one, every time.

## Reporting problems

- **A desync or a replay that will not reproduce** is the highest-value bug report there is. Include
  the map seed, the playbooks, the operating system, and the tick where the hash chain first
  diverges.
- **Security issues** in the gateway (a bind address, a scope leak, a token that outlives its match)
  go privately to the owner first, not into a public issue.
