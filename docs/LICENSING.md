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
| The game | `crates/**` (except `crates/proto/**`), `xtask/**`, `godot/**`, `client/**`, `scenarios/**`, `tests/**` (except the four golden areas in the next row and `tests/LICENSE`), and the build configuration (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `clippy.toml`, `deny.toml`, `.cargo/**`, `.github/**`, `scripts/**`, `.gitignore`, `.gitattributes`, `README.md`) | `GPL-3.0-or-later` | `LICENSES/GPL-3.0-or-later.txt` |
| Interfaces and words | `proto/**`, **`crates/proto/**`**, `schemas/**`, `docs/**`, `examples/**`, `library/**`, `rules/**`, `packaging/**` (but for its two licence texts, below), **`tests/golden/proto/**`**, **`tests/golden/plan-core/**`**, **`tests/golden/schema/**`**, **`tests/golden/operator/**`**, `llms.txt`, `AGENTS.md`, `CLAUDE.md`, `CHANGELOG.md`, and the repository metadata (`LICENSE`, every per-directory `LICENSE` pointer, `REUSE.toml`, `examples/.gitkeep`) | `MIT OR Apache-2.0` | `LICENSES/MIT.txt`, `LICENSES/Apache-2.0.txt` |
| Art and audio | `assets/**` | `CC-BY-SA-4.0` | `LICENSES/CC-BY-SA-4.0.txt` |

**The one carve-out inside `crates/`:** `crates/proto` holds the generated
`gp.v1` / `gp.api.v1` types, which are the same public surface as the `.proto`
files themselves, so it is `MIT OR Apache-2.0` — set deliberately in
`crates/proto/Cargo.toml`, stated in that crate's SPDX headers, and carved out
of `crates/**` by a later annotation in `REUSE.toml`. Nothing else under
`crates/` is permissive.

**The same carve-out inside `tests/`, four times over:**
`tests/golden/<area>/expected.*` is the committed half of the harness — hash
chains, canonical playbooks, verifier reports, path hashes, geometry digests,
feed logs and vista PNGs — produced by the game's own code and read only by it,
so it takes the game's licence. Four areas are the exception, because a third
party writing a playbook tool needs them as the reference they are:
`tests/golden/proto/**` (canonical `gp.v1` JSON and views of the schema, the
same public surface as `proto/**`), `tests/golden/plan-core/**` (the canonical
form of the example playbooks), `tests/golden/schema/**` (the generated JSON
Schema `get_schema` serves) and `tests/golden/operator/**` (the playbooks the
built-in operator seals from `library/`'s permissive templates) — decisions log
§2.7 items 100 (11) and 113. `REUSE.toml` carves each out with a later
annotation, and `tests/LICENSE` says so in prose.

**A file that quotes `library/`.** This is the standing statement of the rule; every other
file points here. A tracked file outside `library/` that reproduces a library template's
expression carries the library's licence (`MIT OR Apache-2.0`) beside the game's, as one
combined expression, `GPL-3.0-or-later AND (MIT OR Apache-2.0)`, or, where the file carries no
game expression at all, the library's licence alone. The `docs/` tree is permissive by its own
tier, not because it is library text. The rule was recorded in decisions log §2.7 item 164,
which amends item 113 (10).

*What counts as expression.* A sentence of at least 8 words ending in a full stop, or a run
of at least 12 consecutive words of one library string, full stop or not (a parameter's
description or a long label has no full stop and is expression). The two numbers are the
constants `MIN_WORDS` (8) and `RUN_WORDS` (12) in `xtask/src/licence_quotes.rs`. A title, a
name or a short step label is below both and is not covered. The line is the main session's,
not a finding about copyright: below it the project treats a file as quoting no expression,
a contributor may always add the library's licence to a file below it (the check demands the
licence only above it), and the owner can move the line.

*What passes.* `reuse` often reports more than one expression for a file (one from a manifest
block, one from the file's header; `crates/gateway/src/host.rs`, `crates/client-gdext/tests/wizard.rs`,
`scenarios/skeleton/against-easy.playbook.jsonc` and `tests/golden/vista/README.md` each have both).
The check requires each expression a quoting file carries to be exactly one of three forms,
`GPL-3.0-or-later`, `MIT OR Apache-2.0` or `GPL-3.0-or-later AND (MIT OR Apache-2.0)`, compared
as written (`OR` where `AND` is meant fails). The file passes if the combined form is among its
expressions, or if it carries `MIT OR Apache-2.0` and no `GPL-3.0-or-later`. `GPL-3.0-or-later`
and `MIT OR Apache-2.0` as two separate expressions, with no combined one, fails. The check does
not test the area a file sits in: a quoting file passes with the library's licence alone
wherever it is, provided it carries no game expression. A file under a game-tier manifest block
always carries one from the manifest, which is what keeps the library's licence alone to the
permissively licensed areas.

*The converse.* The check takes every expression `reuse` reports for a file together, as the set
of licence identifiers they name, operators dropped. A tracked file outside `library/` whose set
holds the same three licence identifiers as the combined expression, `GPL-3.0-or-later`, `MIT`
and `Apache-2.0`, and which quotes nothing of the library, fails: it has no reason to carry the
library's licence, and is `GPL-3.0-or-later`. That holds whatever else it carries beside them (a
fourth licence, such as `... AND CC-BY-SA-4.0`, does not excuse it), whatever joins them (the
order swapped, `OR` where `AND` is meant), and however they are split across its header, a
sidecar and manifest blocks (a block's `GPL-3.0-or-later` beside a header's `MIT OR Apache-2.0`
fails like the combined form). A media, font or descriptor file (the first extension list below)
is never read and holds no prose, so it fails if its set holds the three. The converse needs the
game's licence in the set: a file that carries `MIT OR Apache-2.0` with no `GPL-3.0-or-later`
beside it and quotes nothing passes, because the permissive areas are permissive by their own
tier. This is what turns a forgotten header red, the library's licence left on a file after its
quotation was removed (decisions log §2.7 item 164 (3a)). Files under `library/` are the text
quoted, not files that quote it, and are not held to the converse.

*What the check reads from `library/`.* Every tracked file under `library/` that `reuse`
reports; a file that `reuse` does not report because it ignores it (one whose name starts with `LICENSE`,
`LICENCE` or `COPYING`, or ends `.license`; a licence text under `LICENSES/`; `REUSE.toml`; an
empty file) is not read. In a `.json` or
`.jsonc` file the strings are the *library strings* and the `//` comment lines are comment
paragraphs (consecutive whole-line comments form one paragraph, a blank comment line ends it,
and a trailing comment is a paragraph of its own); a string or paragraph that contains
`SPDX` is skipped. Sentences come from strings and comment paragraphs; 12-word runs come from
strings only. Any other library file (a Markdown note) is read by its paragraphs, which give
both. It reads `library/`'s current text on every run, so a changed template changes what is
covered.

*Which files, and how words are counted.* Every file `git ls-files` lists outside `library/`,
except media, font and descriptor files (`png`, `jpg`, `jpeg`, `gif`, `ico`, `vox`, `ogg`,
`wav`, `mp3`, `ttf`, `otf`, `woff`, `woff2`, and `binpb`, the descriptor set `buf` writes from
`proto/` with source info excluded), which hold no prose and are never read, and the files
`reuse` does not report because it ignores them (the list above). A tracked archive or
executable (`zip`, `pck`, `wasm`, `dll`, `so`, `dylib`, `exe`, `bin`) fails the check whatever its
licence: it can embed library text that the check cannot read, and a build output is not
tracked. A tracked file missing from disk fails the check, and so does any other file that
cannot be read or is not UTF-8 text, and a file `reuse` should report and does not. The 8-word minimum counts whitespace-separated tokens
of a library string or comment paragraph (whitespace collapsed), punctuation included. A
sentence ends at a full stop followed by a space, or at a full stop that ends the text; the
text is split at every such full stop, so "e.g. " ends a piece, and a piece counts only if it
ends in a full stop and has at least 8 tokens. A sentence is matched as written,
punctuation included, as a substring of the file's text. A 12-word run counts runs of letters
and digits, punctuation dropped, and is matched the same way. Text is compared with whitespace
collapsed, JSON string escapes undone (all of them, Unicode escapes included) and comment markers
removed, so a re-wrapped comment, an escaped quote or an escaped newline inside a sentence still matches.
A byte copy under `godot/fixtures/` of a file under `tests/golden/` or `scenarios/` must carry
exactly the same expressions as its source.

*Library text embedded in the GPL binaries.* The game's binaries embed library text at compile
time (the gateway's safe playbook, which `gamectl` carries). A `GPL-3.0-or-later` binary may embed
text under `MIT OR Apache-2.0`, which those licences permit, and the zips ship both licence texts
(see "What a zip carries"). This holds whether the text is a hand-kept copy in the source or
`library/safe_playbook.jsonc` embedded by `include_str!` (decisions log §2.7 item 177 (9)).

*Which mechanism.* The file's own SPDX header where the file can carry one; a `.license`
sidecar or a `REUSE.toml` block where it cannot (JSON, a byte copy, a hash chain); an
`override` entry only where the file embeds another file's SPDX header, as the gateway's
`instantiate_suggested` answer and its Godot copy do, because `reuse` would otherwise read the
embedded header as the file's own. An override is the last word for its paths, so a change of
licence there is an edit to the block.

*Where it runs and where it fails.* In the `reuse` step of `cargo xtask ci` (`cargo xtask
reuse`), after `reuse lint`, so it also runs on the docs-only fast path. It asks `reuse lint
--json` for each file's expressions and reads no manifest by hand. It has no exemption for an
override block and no allow table. A failure names each file, the first words it quotes and
the licence to write. This is the usual wording, one of several:

    these files' licences disagree with what they quote of library/ (MIT OR Apache-2.0);
    each line names the licence to write (REUSE.toml, or the file's own header):
      <file> quotes "<first 60 characters>..." and is licensed `GPL-3.0-or-later` only; write
      `GPL-3.0-or-later AND (MIT OR Apache-2.0)`

Other file lines end differently, as the case needs:

    ... and carries <X>; write <the combined form>          (the combined form in another order)
    ... and carries <X>, which is none of the three forms
    ... and carries GPL-3.0-or-later and MIT OR Apache-2.0 as two separate expressions;
        give it the combined expression: write <the combined form>
    <copy> is a byte copy of <source>, which is licensed [...], not [...]
    <file> carries `<X>` and `<Y>` and quotes no sentence and no word run of library/, so
        the library's licence has no reason to be there: write `GPL-3.0-or-later` (its own
        header), or delete the REUSE.toml entry that gives it the library's licence
                                                            (the converse; each expression once)
    <file> carries `<X>` and is a media, font or descriptor file, which holds no library/
        prose, so ...                                       (the converse, for such a file)

Other refusals say that `reuse lint --json` did not run or gave an unreadable answer, that a file
is not readable as text (add its extension to the media list, or to the archive list), that a
tracked file is not on disk (restore it, or commit its removal), that a file could not be read
from disk, that a tracked file is an archive or executable (untrack it), that `reuse`'s report
has no entry for a file, or that the positive control failed.

A positive control keeps "nothing found" from meaning "nothing looked at": the check fails
unless it read more than 800 files, at least 5 library files and more than 100 word runs, and
found each of eleven named files (the six template render goldens, the `instantiate_suggested`
pair, `godot/fixtures/rule_list_render.json`, `scenarios/skeleton/against-easy.playbook.jsonc`
and `crates/gateway/src/host.rs`) quoting the library and carrying the game's licence and the library's, so
the converse, which keys on those licences, cannot pass by finding nothing either.

**`rules/`** is `MIT OR Apache-2.0` for the reason `proto/` is: `rules.v1.json`
is the canonical JSON of one `gp.v1.RulesTable` (decisions log §2.7 item 78) —
the shape is schema and the values are data — and an alternative verifier or
planner has to read the same table to reach the same estimates. The JSON cannot
carry a header, so `rules.v1.json.license` sits beside it.

**`library/`** is `MIT OR Apache-2.0` too: the template library is playbook
files, data a player copies and changes, and nothing in it executes, so it takes
the example playbooks' licence (decisions log §2.7 item 111). The files carry
the header themselves, and `library/LICENSE` is the pointer.

**`packaging/`** is `MIT OR Apache-2.0` like the documentation: it holds what
`cargo xtask package` writes into the unsigned zips from templates, and
`packaging/LICENSE` is the pointer (decisions log §2.7 item 117 (11)). See
[What a zip carries](#what-a-zip-carries) below.

Three kinds of path are **not** covered by any tier: `docs/DCO.txt`, which is
the Linux Foundation's document and keeps its own verbatim-copy terms (see
[Contributions](#contributions-dco-sign-off) below); the canonical MPL-2.0 and
BSL-1.0 texts in `packaging/` (`LICENSE-MPL-2.0.txt`, `LICENSE-BSL-1.0.txt`),
which keep their own terms and are there only to be copied into the zips; and
any third-party asset, which keeps its upstream licence in a `.license` sidecar.

<!-- REUSE-IgnoreStart: prose, not a copyright notice; this file's own is its header -->
Copyright holder: **Pharmakos contributors**. Copyright year: **2026**.
<!-- REUSE-IgnoreEnd -->

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
game, but GPL'd code must not be copied into `proto/`, `schemas/`, `docs/`,
`examples/`, `library/`, `packaging/` or `rules/`. When in doubt, a `.proto` file must be written from the spec, not
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

<!-- REUSE-IgnoreStart: an example sidecar, not this file's own -->
```
SPDX-FileCopyrightText: 2019 Some Author <https://freesound.org/people/someauthor/>
SPDX-License-Identifier: CC0-1.0
SPDX-FileComment: https://freesound.org/s/123456/ — trimmed to 0.4 s, normalised.
```
<!-- REUSE-IgnoreEnd -->

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

<!-- REUSE-IgnoreStart: an example header, not this file's own -->
```rust
// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```
<!-- REUSE-IgnoreEnd -->

The identifier must match the tier for that path. Files that cannot hold a
comment use a `.license` sidecar. Generated files inherit the tier of the
directory they are generated into, and the generator should emit the header.

`REUSE.toml` uses `precedence = "aggregate"`, which means a header inside a
file is combined with the manifest entry rather than being overridden by it —
so a third-party file with its own header keeps it.

Conformance is checked with `reuse lint`, which runs as the `reuse` step of
`cargo xtask ci`, followed by the quotation check described above. The check is platform-independent, so in CI it runs on the
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
asked for one before it is merged. Before you push, fix it in place with
`git commit --amend -s`, or `git rebase --signoff <base>` for several commits;
once a branch is pushed it is never rewritten, so make the commits again,
signed, on a new branch and open a new pull request (AGENTS.md sections 6 and 8).

By signing off you agree that your contribution is licensed under the tier of
the directory it lands in, as listed in the table above, with no additional
terms.

## What a zip carries

`cargo xtask package` builds the unsigned zips (decisions log §2.7 items 117 (11)
and 121), and each carries its own licensing, which the command checks with
`reuse lint` inside the extracted zip before CI uploads it:

- **Its own `REUSE.toml`**, written from `packaging/REUSE.toml.in`, which is
  named `.in` so that this repository's `reuse` does not read it as a nested
  manifest. The game executable is Godot's export template, so it is recorded
  as MIT with the Godot Engine contributors' own lines; the engine's
  third-party components are the ones the Credits screen draws from the engine
  itself (`Engine.get_copyright_info`, `Engine.get_license_info`). The `.pck`,
  the game's scenes and compiled scripts, is `GPL-3.0-or-later`. The client
  library and `gamectl` are recorded with `GPL-3.0-or-later` and the licences of
  the crates compiled into each, computed from `cargo tree`. `rules/`,
  `library/`, the note (`README.txt`), `CHANGELOG.md` and the notices file are
  `MIT OR Apache-2.0`.
- **A `LICENSES/`** holding exactly the licences those records name, because
  `reuse` fails an unused one: Apache-2.0, GPL-3.0-or-later and MIT from this
  repository's `LICENSES/`, and MPL-2.0 (the godot-rust crates in the client
  library) and BSL-1.0 (xxhash-rust in `gamectl`) from `packaging/`. `reuse`
  ignores `LICENSE*` names, so this repository neither lints those two texts
  nor counts them as licences it uses, and nothing is added to its own
  `LICENSES/`.
- **`THIRD-PARTY-NOTICES.txt`**, generated from `cargo tree` over the crates
  linked into the two binaries, and the godot-rust build-time crates whose
  generated code is compiled in: each crate's name, version, licence and
  source, then its own licence and notice files as its package ships them, but
  with LF line endings and without trailing blank lines. The godot-rust crates
  ship no licence file, so their entries name MPL-2.0 and say where the source
  is (MPL-2.0 section 3.2). The file also names Rust's standard library and, on
  Windows, the statically linked C runtime.

The note's source line names the commit a zip was built from; the repository's
public address joins it before a release is published (a PLACEHOLDER in
`packaging/README.md`, the owner's, with the org).

## Changing the licensing

The licence files, `REUSE.toml` and the per-directory `LICENSE` pointers are
**contract files**: they change only with owner approval, the same as `.proto`
files, the lint set and the determinism rules. Adding a new top-level directory
means adding a `REUSE.toml` entry for it in the same change, and a `LICENSE`
pointer too when its files are not in the game tier: the root, `assets/`,
`docs/`, `examples/`, `library/`, `packaging/`, `proto/` and `rules/` have one.
`tests/`, in the game tier, has one as well, naming its permissive golden areas;
`crates/`, `godot/`, `scenarios/`, `scripts/`, `xtask/`, the dot-directories and
`LICENSES/` have none. Adding a new licence to `LICENSES/` means checking that it is compatible
with the tier it will sit beside — for the game tier, that means compatible with
GPL-3.0-or-later.
