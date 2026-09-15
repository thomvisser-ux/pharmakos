<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `plan-core/` — canonical form, the JSONC round trip, and prose

**Filled by T8** (`crates/plan-core`).

Three kinds of golden live here, and they fail for different reasons:

* `expected.jsonc` — a hand-written playbook reproduced **byte for byte**
  through load, canonicalise and save, with comments and formatting intact. This
  is the property the whole editor model rests on (spec §13, "Exact
  round-trip").
* `expected.canonical.json` — the canonical form the writer emits.
* `expected.prose.txt` — `render_plan`'s English rendering.

## What a diff means

* **The JSONC round trip moved.** A comment moved, was dropped, or whitespace
  changed. This is never cosmetic: the editor promises the player that saving a
  file they wrote by hand gives the file back. A dropped comment is a bug even
  when the JSON is identical.
* **The canonical form moved.** Key order, number formatting or default-value
  handling changed. `report_hash` is a pure function of the playbook bytes, so a
  canonical-form change moves every verifier golden with it — if this moved and
  `verifier/` did not, one of the two is wrong.
* **The prose moved.** Either a template string changed (say so; it is the one
  the player reads) or the plan itself renders differently, which is a semantic
  change wearing a typographic disguise. `render_plan` reads the coming
  segment's length from the frozen snapshot, so a prose diff after a segment
  ladder change is expected and should be stated.
