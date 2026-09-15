<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `docs/` — generated documentation output

**Filled by T13** (`gamectl docs` and the gateway's `docs` scope).

The documentation the gateway and the CLI generate from the schema and the
diagnostic catalogue, pinned so it cannot drift from its sources.

## What a diff means

* **A source moved.** The schema, a diagnostic's message or a beginner sentence.
  The pull request that moved the source carries this diff; they never arrive
  separately.
* **Only the layout moved.** A template change. Harmless, and worth saying so in
  one line so the reviewer does not go looking for a semantic change.
* **A code or a method disappeared.** Documentation is the published surface a
  script author reads at v1.1. A removal is a breaking change even when the code
  behind it still exists.
