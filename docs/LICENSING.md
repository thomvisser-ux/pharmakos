<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Licensing

Pharmakos ships three licence tiers, chosen so that the game is copyleft while
everything you need in order to *talk to* the game is permissive. This document
is the prose explanation; `REUSE.toml` at the repository root is the
machine-readable source of truth, `LICENSE` at the root is the short pointer,
and the full texts live in `LICENSES/` under their canonical SPDX file names.

| Tier | Paths | Licence | Full text |
| --- | --- | --- | --- |
| The game | `crates/**` (except `crates/proto/**`), `xtask/**`, `client/**` | `GPL-3.0-or-later` | `LICENSES/GPL-3.0-or-later.txt` |
| Interfaces and words | `proto/**`, **`crates/proto/**`**, `schemas/**`, `docs/**`, `examples/**`, `llms.txt`, `AGENTS.md`, `CLAUDE.md` | `MIT OR Apache-2.0` | `LICENSES/MIT.txt`, `LICENSES/Apache-2.0.txt` |
| Art and audio | `assets/**` | `CC-BY-SA-4.0` | `LICENSES/CC-BY-SA-4.0.txt` |

**The one carve-out inside `crates/`:** `crates/proto` holds the generated
`gp.v1` / `gp.api.v1` types, which are the same public surface as the `.proto`
files themselves, so it is `MIT OR Apache-2.0` — set deliberately in
`crates/proto/Cargo.toml`, stated in that crate's SPDX headers, and carved out
of `crates/**` by a later annotation in `REUSE.toml`. Nothing else under
`crates/` is permissive.

Two paths are **not** covered by any tier: `docs/DCO.txt`, which is the Linux
Foundation's document and keeps its own verbatim-copy terms (see
[Contributions](#contributions-dco-sign-off) below), and any third-party asset,
which keeps its upstream licence in a `.license` sidecar.

Copyright holder: **Pharmakos contributors**. Copyright year: **2026**.

## Why this split

**The game is GPL-3.0-or-later.** Everything that executes in v1 is Rust: the
sim, plan-core, the verifier, the operator, the gateway, `gamectl`, the build
and CI tooling in `xtask/`, and the Godot client with its thin gdext crate. A
fork that ships a modified sim has to ship its modifications. Because the whole
value of a deterministic RTS is that anyone can re-run a match and get the same
answer, a closed fork of the sim would be a fork nobody could audit.

`-or-later` rather than bare `GPL-3.0`: if the FSF publishes GPLv4, downstream
users can move without tracking down every contributor.

**The interfaces are `MIT OR Apache-2.0`.** The `.proto` files (`gp.v1`,
`gp.api.v1`), the JSON Schema generated from them, the documentation, the
example playbooks and `llms.txt` are the public surface. Somebody writing a
third-party playbook generator, an SDK, an alternative editor or a lint tool
should not be forced to adopt the game's licence to do it. The SPDX `OR` is a
disjunction — the user picks either licence — which is the Rust ecosystem's
default expectation and lets the schemas be vendored into permissive projects.
Apache-2.0 is in the pair for its explicit patent grant; MIT is in the pair
because some downstreams still prefer the shorter text.

Note the one-way consequence: permissive material can be pulled into the GPL'd
game, but GPL'd code must not be copied into `proto/`, `schemas/`, `docs/` or
`examples/`. When in doubt, a `.proto` file must be written from the spec, not
pasted out of a crate.

**Art and audio are CC BY-SA 4.0.** Creative Commons licences are the ones
artists actually use, and CC BY-SA 4.0 is the copyleft member of that family,
so an adaptation of our art stays open the way an adaptation of our code does.

### Why CC BY-SA 4.0 is safe next to GPLv3

CC BY-SA 4.0 is **one-way compatible** into GPL-3.0-or-later. Section 3(b)(1)
of the BY-SA 4.0 legal code lets the adapter's licence be "a BY-SA Compatible
License", which section 1(c) defines as one listed at
`creativecommons.org/compatiblelicenses` and approved by Creative Commons as
essentially equivalent. GPLv3 is on that list. So material licensed BY-SA 4.0
may be relicensed under GPLv3 when it is adapted — which is what happens when a
`.vox` model or an `.ogg` is packed into the shipped game. The arrow only points one way. GPL'd material
must never be relicensed back into `assets/` as BY-SA. That asymmetry is the
reason art lives in its own directory instead of being embedded in source
files: the directory boundary is the licence boundary, and `REUSE.toml` records
it.

The practical consequence for a reuser: if you take an asset on its own, CC
BY-SA 4.0 applies (attribute, note changes, share alike). If you take the built
game, GPL-3.0-or-later applies to the whole.

## Audio sourcing rule

Audio may be drawn **only from CC0 or CC-BY sources**. Nothing else enters the
repository, no matter how good it sounds.

Allowed:

- **Kenney** asset packs (CC0).
- **Freesound**, restricted to the CC0 and CC-BY filters. A clip under CC BY-NC,
  CC BY-ND or the Freesound sampling licences is not usable.
- **sfxr / ChipTone** output — generated here, so it is ours and CC0-equivalent
  before it is relicensed BY-SA as part of `assets/`.
- **Music**: two or three tracks from OpenGameArt, ccMixter or Opsound, each
  with an individual per-track licence check recorded in its sidecar file.
  Shipping v0.1 with no music at all is an acceptable outcome.

**Sonniss GDC bundles are excluded.** They are distributed under a custom
licence, not a Creative Commons one, and the v2.0 terms additionally forbid use
in AI training. That is incompatible with putting the files in a BY-SA
repository and with a project whose whole subject is machine operators. Do not
download them into this tree, and do not "just use one for a placeholder".

Also excluded by the same rule: anything CC BY-NC (non-commercial is not a free
licence), anything CC BY-ND (no derivatives, so it cannot be adapted), and
anything whose licence cannot be established from the source page. "I could not
find a licence" means no.

### Recording a third-party asset

Third-party files keep their own copyright and licence; the blanket
`assets/** → CC-BY-SA-4.0` entry in `REUSE.toml` does **not** apply to them.
Because a `.vox` or an `.ogg` cannot carry a comment header, REUSE's sidecar
convention is used: next to `shot.ogg`, add `shot.ogg.license` containing

```
SPDX-FileCopyrightText: 2019 Some Author <https://freesound.org/people/someauthor/>
SPDX-License-Identifier: CC0-1.0
SPDX-FileComment: https://freesound.org/s/123456/ — trimmed to 0.4 s, normalised.
```

and add the licence text to `LICENSES/` if it is not there yet (`CC0-1.0.txt`
for CC0 material, `CC-BY-4.0.txt` for CC BY material). Every CC-BY asset must
also appear on the in-game **credits screen**, which is where the attribution
requirement is actually discharged for a player who never opens the repository.
CC0 assets need no attribution but are still recorded in the sidecar so that
provenance is auditable.

## Tooling authoring note

`assets/` holds the outputs, not the editors. Models are authored in
MagicaVoxel (freeware, closed source; models belong to their author) or Goxel
(GPL), and loaded at runtime with `dot_vox` (MIT) through our own greedy
mesher. Neither authoring tool's licence attaches to the `.vox` files it
produces.

Rust dependencies keep their own licences and must be GPL-3.0-compatible; the
common `MIT OR Apache-2.0` crate licensing is. That is checked, not assumed:
`cargo deny check` runs as the `deny` step of `cargo xtask ci` against
`deny.toml`, which holds the GPL-compatible allow-list, the advisory database
settings and the five banned crates (`bincode`, `cordic`,
`hierarchical_pathfinding`, `rmcp`, `wasmi`). `.github/workflows/ci.yml`
installs cargo-deny on all three runners, and `PHARMAKOS_REQUIRE_TOOLS=1` there
turns a missing tool into a failure rather than a skip. `deny.toml` is the
contract file: changing the allow-list needs owner approval.

## SPDX headers

Every text file that can hold a comment carries a two-line header, so a file
that has been copied out of the tree still says what it is:

```rust
// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

The identifier must match the tier for that path. Files that cannot hold a
comment use a `.license` sidecar. Generated files inherit the tier of the
directory they are generated into, and the generator should emit the header.

`REUSE.toml` uses `precedence = "aggregate"`, which means a header inside a
file is combined with the manifest entry rather than being overridden by it —
so a third-party file with its own header keeps it.

Conformance is checked with `reuse lint`, which runs as the `reuse` step of
`cargo xtask ci`. The check is platform-independent, so in CI it runs on the
Linux leg only: the Windows and macOS legs pass `--skip reuse`
(`.github/workflows/ci.yml`). Locally the step reports `skipped` when the tool
is not installed (`pipx install reuse`); in CI it cannot, because
`PHARMAKOS_REQUIRE_TOOLS=1` is set.

## Contributions: DCO sign-off

Contributions are accepted under the **Developer Certificate of Origin 1.1**,
reproduced verbatim in [`DCO.txt`](DCO.txt). There is no CLA and no copyright
assignment: you keep your copyright, and the DCO is your statement that you
have the right to submit what you are submitting.

Sign off every commit:

```
git commit -s
```

which appends

```
Signed-off-by: Your Name <your.email@example.com>
```

using your real name and a working address. A commit without a sign-off will be
asked for one before it is merged; the fix for an existing branch is
`git rebase --signoff <base>`.

By signing off you agree that your contribution is licensed under the tier of
the directory it lands in, as listed in the table above, with no additional
terms.

## Changing the licensing

The licence files, `REUSE.toml` and the per-directory `LICENSE` pointers are
**contract files**: they change only with owner approval, the same as `.proto`
files, the lint set and the determinism rules. Adding a new top-level directory
means adding a `REUSE.toml` entry and a `LICENSE` pointer for it in the same
change. Adding a new licence to `LICENSES/` means checking that it is
compatible with the tier it will sit beside — for the game tier, that means
compatible with GPL-3.0-or-later.
