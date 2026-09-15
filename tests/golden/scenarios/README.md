<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `scenarios/` — what the scenario runner asserts on

**Filled by T11** (the first full-segment hash chain) and T15 (the runner).

One directory per scenario, named after its `name` key:

```text
tests/golden/scenarios/<name>/expected.hashes.txt   the per-tick chain
tests/golden/scenarios/<name>/expected.events.txt   the event log it asserts on
```

The chain has the same format as `determinism/`: tick, TAB, sixteen lowercase
hex digits, one line per tick, LF, trailing newline. `scenarios/README.md`
documents the scenario files themselves.

## What a diff means

* **The chain moved.** Read `determinism/README.md` — it is the same news, on a
  seed that a scenario happens to name.
* **The event log moved but the chain did not.** Impossible if the events are
  derived from hashed state, so this is either an event that is *not* hashed
  state (fine, and it should say so) or a bug. Decide which before blessing.
* **The chain moved but the event log did not.** The sim's behaviour changed in
  a way no assertion in this scenario covers. That is worth a moment: the
  scenario may be asserting less than it should.
* **A scenario passed after an assertion was loosened.** The pull request must
  say so out loud. A weakened check reads exactly like a fixed bug in the
  summary line and nowhere else.
