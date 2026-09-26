<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later

CONTRACT FILE (AGENTS.md §5): golden-file formats. Changes need owner approval.
The machine-checkable half lives in xtask/src/golden.rs.
-->

# Golden files

A golden is a committed, reviewed, human-diffable output. `cargo xtask ci`'s
`golden` step byte-compares every committed one against a fresh one, and a
golden that moves is a **behaviour change that has to be explained in the pull
request that moved it** (AGENTS.md §5, §10 item 2).

## The convention

```text
tests/golden/<area>/README.md            what a diff in this area means
tests/golden/<area>/<case>/expected.*    committed
<target>/golden/<area>/<case>/actual.*   written fresh by the test that produces it
```

The producing test writes `actual.<ext>` under Cargo's target directory as it
runs — `<target>` honours `CARGO_TARGET_DIR`, so it is wherever cargo put
everything else. The step then compares the pair.

```sh
cargo xtask ci                # compares
cargo xtask golden --bless    # accepts the fresh outputs as the new goldens
```

## Four rules

1. **A missing fresh output is a failure, not a skip.** A golden with nothing to
   compare against checks nothing. A step that reports `ok` for work it did not
   do is the failure this harness exists to prevent (spike G1 §10.12).
2. **Every area carries a `README.md` saying what a diff there means.** A moved
   hash chain, a moved `report_hash` and a moved prose golden are three
   different kinds of news, and the person reading the red build is usually not
   the person who wrote the golden. The `golden` step *fails* an area that has
   goldens and no README.
3. **Goldens are human-diffable** (AGENTS.md §9 item 7): text where text will
   do, one record per line, LF endings, a trailing newline, no `\r` — they are
   byte-compared across Windows, Linux and macOS. The one exception is the vista
   PNG, compared with a tolerance by `xtask/src/png.rs` rather than byte for
   byte, and it says why in its own README.

   The endings half is **enforced**, and it has to be, because nothing else in
   the toolchain can enforce it. `.gitattributes` normalises the repository with
   `* text=auto eol=lf` and then exempts this tree with `tests/golden/** -text`,
   deliberately: git must not rewrite a file whose bytes are the assertion. The
   price is that a CRLF file committed here stays CRLF, in the one directory
   where that matters most, and git, `reuse` and rustfmt all have nothing to
   say about it. So the `golden` step checks every file under `tests/golden/`
   that is valid UTF-8 — READMEs included — and refuses a `\r` or a missing
   final newline. A file that is not valid UTF-8 is binary and is left alone; a
   PNG's signature contains a `\r` and must keep it.

   `--bless` is not the fix for this one: it copies the fresh output over the
   golden, so a producer writing CRLF writes it again. Fix the producer.
4. **Never re-bless a golden to turn a red test green** without writing down the
   behaviour change that moved it (AGENTS.md §5). `--bless` prints a reminder;
   the pull request is where it is honoured.

## The two areas the `golden` step does not compare

`determinism/` and `vista/` are compared by the steps that produce them, and the
`golden` step leaves both alone. The list is `SELF_COMPARED_AREAS` in
`xtask/src/golden.rs`, and it is the only place they are named.

| Area | Compared by | Re-baselined with |
| --- | --- | --- |
| `determinism/` | the `determinism` step, which runs the sim, validates the chain's format line by line and says which tick first diverged | `cargo xtask determinism --bless` |
| `vista/` | the `screenshot` step, through `xtask/src/png.rs`, with a tolerance | a deliberate re-render at T16; see `vista/README.md` |

Rule 1 is what makes this necessary rather than tidy. The `golden` step runs
*before* both, so on a clean checkout neither has produced anything yet, and "a
missing fresh output is a failure" would fail on all three operating systems —
permanently so for `vista/`, which is rendered on the Linux leg alone. Rule 2
still holds for both: each carries its README, and the `golden` step still fails
an area that does not.

## The areas, and who fills each

The layout is frozen at the skeleton so that later stages add *files*, never
formats (plan §5). Each directory below carries its own README now; the task
named fills it.

| Area | What it pins | Filled by |
| --- | --- | --- |
| `determinism/` | The per-tick xxh3 state-hash chain | T2 |
| `pathing/` | Route hashes over the pristine map plus crater repairs | T7 |
| `proto/` | `gp.v1` canonical-JSON round trips | T1 |
| `plan-core/` | Canonical form, byte-exact JSONC round trip, `render_plan` prose | T8 |
| `verifier/` | Reports, `report_hash`, the diagnostic catalogue | T6 |
| `gateway/` | The fog filter, the segment digest, the audit log and the handshake | T9 |
| `interpreter/` | Decision, step-transition and commit-point transcripts | T11 |
| `economy/` | `$` and `kW` ledgers and the BMI settlement | T14 |
| `mapgen/` | Per-seed map digests | T5 |
| `mesher/` | Per-chunk vertex and index digests | T4 |
| `schema/` | Generated JSON Schema and `get_schema` output | T13 |
| `docs/` | Generated documentation output | T15, T20 |
| `scenarios/` | Hash chains and event logs the scenario runner asserts on | T11, T15 |
| `operator/` | The playbook the built-in operator seals for each seat, per seed | T18 |
| `vista/` | The rendered screenshot, compared with a tolerance | T16 |

## Licences

Goldens take the game's licence, GPL-3.0-or-later, because they are records of
what the game's own code did and are read only by it. `proto/` was the first
carve-out: its files are canonical `gp.v1` JSON and views of the schema — the
same public surface as `proto/**` — so REUSE.toml puts that directory in the
permissive tier and its README carries the permissive header. `plan-core/` and
`schema/` have since joined it on the same argument (decisions-log item
100 (11)), and `operator/`, whose sealed playbooks come from `library/`'s
permissive templates, joined them (decisions-log item 113); `docs/` will raise the question again when it lands, and the
PLACEHOLDER in REUSE.toml records it against T20.

Most of these formats cannot carry an SPDX header at all — a hash chain is
`<tick>\t<hash>` per line and nothing else, and a PNG has nowhere to put one —
so REUSE.toml covers them and only the READMEs carry headers of their own.

## What is not here

Perf numbers. A figure that cannot vary across platforms must not be compared
across them, and a figure that *can* vary is not a golden — G3′ §9.17's warning
about a green matrix that means nothing. Performance is published as
`::notice::` annotations per runner, with no threshold at this stage
(skeleton-plan §7 decision 23, recommended and not yet logged; AGENTS.md §9
item 11 puts budgets with the gates that set them).
