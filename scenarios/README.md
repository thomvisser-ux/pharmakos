<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later

CONTRACT FILE (AGENTS.md §5): this is a golden/harness format. Changes need
owner approval, and the machine-checkable half lives in xtask/src/scenario.rs.
-->

# Scenario files

**A scenario is a headless match written down.** Map seed, rules table, one
playbook per seat, the segment list, and assertions on events **and** on the
hash chain. The first three are AGENTS.md §4's triple — *"the sim is a pure
function of (map seed, playbooks, rules hash)"* — which is what makes a
committed hash chain a claim about the sim rather than about whatever happened
to be checked out that afternoon.
`gamectl scenario run scenarios/<set>/<name>.scenario.jsonc` replays it and
fails on the first assertion that does not hold.

This is what closes AGENTS.md §10 item 4 — *"headless scenario runs pass on
assertions over events **and** hashes, not just 'it didn't crash'"* — and §9
item 10.

## Status

The **format** is frozen (this file and `xtask/src/scenario.rs`). The **runner**
is not written: `gamectl scenario run` arrives at T15. Until then
`cargo xtask ci`'s `scenario` step still reads every file here, validates it
against the format, and reports itself *skipped, with the reason and the task
named*. A malformed scenario is a red build today; a missing runner is a skip.

The formats land first on purpose (decisions-log §2.7 item 75): every later task
delivers *into* a format instead of inventing one, and no golden's shape is
renegotiated under deadline in the last fortnight.

## Layout

```text
scenarios/
  README.md                        this file
  <set>/<name>.scenario.jsonc      one scenario
tests/golden/scenarios/<name>/     the hash chains and event logs it asserts on
```

The suffix `.scenario.jsonc` is how the `scenario` step finds them; a file
without it is not a scenario and is not run.

## The file

JSONC — canonical JSON with `//` and `/* */` comments, exactly like a playbook
(spec §10). A scenario is read far more often than it is written: when a
nightly run goes red the first question is *what was this asserting, and why*,
and the answer belongs in the file rather than in a commit message.

```jsonc
{
  "format": "pharmakos.scenario.v1",
  "name": "expand-east-3min",
  "summary": "One sealed playbook walks the commander east and places a beacon.",

  "map": { "seed": "0x00000000ca5caded", "generator": "skeleton" },
  "rules": "rules/rules.v1.json",

  "seats": [
    { "seat": 0, "kind": "playbook", "playbook": "examples/playbooks/expand_east.jsonc" },
    { "seat": 1, "kind": "safe" }
  ],

  "segments": [ { "index": 0, "length_ms": 180000 } ],

  "assertions": [
    { "assert": "event_fired", "event": "beacon_placed", "seat": 0, "by_tick": 2400 },
    { "assert": "hash_chain_equals",
      "golden": "tests/golden/scenarios/expand-east-3min/expected.hashes.txt" }
  ]
}
```

### Keys

| Key | Required | What it is |
| --- | --- | --- |
| `format` | yes | Exactly `pharmakos.scenario.v1`. A new format is a new string, so an old runner refuses a new file rather than half-understanding it. |
| `name` | yes | The scenario's name. Also the directory its goldens live in. |
| `summary` | no | One sentence for the human reading a red build. |
| `map.seed` | yes | The 64-bit map seed, as `"0x"` + **16 lowercase hex digits**. |
| `map.generator` | yes | The generator's name, so a scenario written against one generation rule is not silently replayed against another. |
| `rules` | no | The rules table, repository-relative. Defaults to `rules/rules.v1.json` — see below. |
| `seats` | yes | One to three seats (spec §3), `seat` counting from 0 in array order. |
| `seats[].kind` | yes | `playbook` (seals the named file), `safe` (files the safe playbook), `builtin` (the built-in operator). |
| `seats[].playbook` | with `playbook` | Repository-relative path, forward slashes, no `..`. |
| `segments` | yes | One or more, `index` counting from 0. |
| `segments[].length_ms` | yes | Game milliseconds, a positive `int32` bare integer (item 46). |
| `assertions` | yes | At least one, and see the pairing rule below. |

**Unknown keys are rejected, never ignored** — the same rule the verifier's Load
applies to a playbook (AGENTS.md §11). A silently stripped assertion is an
assertion that passes by not running.

### The rules table

`rules/rules.v1.json` is the canonical JSON of one `gp.v1.RulesTable`
(decisions-log item 78): K and B, the 10 / 14 / 4 step costs, the move cost per
tick, the repath cap, the segment ladder, the CSR cell size and every `$` and
`kW` number. `pharmakos-sim` loads it and hashes it into `rules_hash`, and the
sim is a pure function of (map seed, playbooks, `rules_hash`). A scenario names
the first two, so it has to be able to name the third.

The key is **optional and defaults to `rules/rules.v1.json`**, because almost
every scenario runs against the shipped table and making every file repeat the
same path is noise that stops being read. The default is checked for existence
like any named one: a scenario silently running against a table that is not
there is the quiet pass this harness exists to prevent.

Name a different table when a scenario is deliberately pinned to one — a tuning
sweep, or a regression that only reproduces at the numbers of the day. Item 78
is explicit that the table's *shape* is a contract file and its *values* are
data that ordinary tuning PRs move during S1 and S2, which is exactly why a
committed hash chain has to record which values produced it. A tuning PR that
moves every scenario's chain is doing so legitimately; one that moves them
without saying so is the failure AGENTS.md §4.8 is about.

### The assertion vocabulary

`pharmakos.scenario.v1` has exactly two:

| `assert` | Keys | Holds when |
| --- | --- | --- |
| `event_fired` | `event`, `by_tick`, optional `seat`, optional `note` | That event was on the bus at or before that tick. |
| `hash_chain_equals` | `golden`, optional `note` | The run's per-tick xxh3 chain equals the committed file, byte for byte. |

`by_tick` is **required**. An assertion with no deadline is satisfied by the end
of the match, which is not an assertion.

`golden` is a repository-relative path under `tests/golden/`, ending
`.hashes.txt`, with forward slashes and no `..` — the same path rule as
`seats[].playbook`. It need not exist yet: the task that first drives the run
commits the chain.

**Every scenario asserts on events AND on the hash chain.** Events alone prove
the match did something; hashes alone prove it did the same thing twice; only
the pair proves it did the right thing reproducibly. The `scenario` step
enforces this.

Three names are **reserved** for the one extension skeleton-plan §7 decision 16
(recommended, not yet logged) schedules for T15, when the runner meets real
events: `event_count_in_range`,
`state_hash_at_tick`, `terminal_hash`. Using one today is an error naming the
task that adds it. The vocabulary is *data inside* the format, so adding to it
is not a format break; removing one would be.

### Writing conventions

These are enforced, not suggested, because a scenario and its goldens are
byte-compared across Windows, Linux and macOS:

* **LF endings, a trailing newline, no tabs.** Indentation is two spaces.
* **The seed is a quoted string, not a number.** JSON numbers are doubles in
  half the world's parsers; a 64-bit seed that round-trips through one loses its
  low bits, and a map that differs between two readers of the same file is not a
  scenario.
* **Durations are bare integers of game milliseconds**, never `"3s"` and never a
  tick count (item 46). Negative is an error with a JSON Pointer at it, exactly
  as the verifier rejects one in a playbook.
* **One assertion per line** where it fits. A diff of a scenario should read as
  "this assertion changed", not "this file changed".
* Every diagnostic carries a **JSON Pointer** (`/seats/1/playbook`), the same
  convention as the verifier's diagnostic catalogue.

## What a diff means

| The diff | What it means | What to do |
| --- | --- | --- |
| A scenario file changed | Somebody changed what is being asserted. | Read the assertion, not the file. If an assertion was *removed* or a `by_tick` *loosened*, the PR must say why — that is a check being weakened. |
| `tests/golden/scenarios/<name>/expected.hashes.txt` moved | The sim's behaviour changed on this seed. | AGENTS.md §4.8 and §5: explain the behaviour change in the PR. Never re-bless a chain to turn a red build green. |
| A scenario newly fails only on one operating system | A determinism hole, not a scenario bug. | Bisect by tick from the first differing line; the usual suspects are unordered iteration, an unchecked cast, a non-total sort comparator, or `usize`/`isize` reaching hashed state. |

## Not in this stage

The three **adversarial** scenarios — Rusher, Turtle, Hunter — and their alarm
bands start at S2 (AGENTS.md §10 item 5) and live in
`.github/workflows/nightly-scenarios.yml`, which stays gated off until then.
The 10 000-playbook fuzz corpus is the verifier's (T6) and runs nightly once its
generator exists. Neither changes this format.
