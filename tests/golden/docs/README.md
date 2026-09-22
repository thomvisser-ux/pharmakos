<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `docs/` — generated documentation output

**Filled by T15** (`gamectl docs`). T13 built the gateway's half of spec §12's
"generated docs" sentence — `get_schema`, whose golden is in `schema/` — and
left this area for the CLI's half.

```text
tests/golden/docs/reference/expected.docs.txt   `gamectl docs`
```

The documentation the CLI generates from the schema and the diagnostic
catalogue, pinned so it cannot drift from its sources. Its four sections have
four different sources and **not one line of it is written by hand**: the
command table and the exit codes are `crates/gamectl/src/cli.rs` and
`exit.rs`, the vocabulary and the enumerations are walked out of the
checked-in `gp.v1` descriptor set, and the diagnostics are the verifier's
`CATALOGUE` and its string table.

Produced by `crates/gamectl/tests/docs.rs` during `cargo test`, compared by
`cargo xtask ci`'s `golden` step, re-blessed with **`cargo xtask golden
--bless`**.

`gp.api.v1` is deliberately absent, and a test asserts it: the method surface
is the transport rather than the vocabulary, and v1 publishes nothing
(AGENTS.md §11 — `llms.txt`, the agent guide and the published schema docs ship
with v1.1).

## What a diff means

* **A source moved.** The schema, a diagnostic's message or a beginner sentence.
  The pull request that moved the source carries this diff; they never arrive
  separately.
* **Only the layout moved.** A template change. Harmless, and worth saying so in
  one line so the reviewer does not go looking for a semantic change.
* **A code or a method disappeared.** Documentation is the published surface a
  script author reads at v1.1. A removal is a breaking change even when the code
  behind it still exists.
* **The reference moved and `schema/`'s golden did not.** The one this area's
  shape exists to catch: both are generated from the same descriptor set, so
  documentation that moves on its own means `docs.rs` has started *building* an
  answer instead of reading one.
* **A message or a beginner sentence changed.** The verifier's string table
  moved. Fine, and the pull request that moved it carries this diff — they
  never arrive separately.
