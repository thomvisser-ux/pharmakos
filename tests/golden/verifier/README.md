<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `verifier/` — reports, `report_hash`, and the catalogue

**Filled by T6** (`crates/verifier`).

* `<case>/expected.report.json` — the full report for one playbook, including
  its `report_hash`.
* `expected.catalogue.json` — the whole diagnostic catalogue, so a code cannot
  be renamed or renumbered in silence.

`report_hash` is a pure function of five inputs: playbook bytes, snapshot, rules
hash, verifier version, depth. Any of the five moving moves the hash.

## What a diff means

* **A `report_hash` moved and the report did not.** One of the other four inputs
  moved — most often the rules hash or the verifier version. Say which.
* **A report moved.** A diagnostic's code, severity, JSON Pointer, message,
  beginner sentence or patch suggestion changed. The pointer and the code are
  what clients bind to; the message is what players read.
* **The catalogue moved.** Code numbers are **append-only from the day they
  ship**. A renamed or renumbered code is a breaking change to every saved
  report and every piece of documentation that quotes it. A code *added* at the
  end is ordinary; anything else needs the pull request to say why.
* **Reports differ between operating systems.** They must not — byte-identical
  across the three is one of T6's acceptance tests. That is a determinism hole
  in the verifier, not a report change.
* **FULL's report grew when S3 fills the lint stage.** Expected, and written
  down in advance (decisions-log item 82): explain it in that pull request.
