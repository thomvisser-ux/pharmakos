<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `pathing/` — route hashes

**Filled by T7** (`crates/sim`: HPA\* and the travel estimator).

`expected.path-hashes.txt` pins the routes the search returns over the pristine
map and after a fixed set of crater repairs — spike G2's shape. One record per
line, LF endings, a trailing newline. Compared across Windows, Linux and macOS:
the route a unit walks is hashed state, so two platforms disagreeing here is a
desync waiting for a match long enough to find it.

## What a diff means

* **The search changed.** Cost model, tie-break or heuristic. The ordering
  `(f, h, node_id)` in a hand-written binary min-heap is part of the determinism
  contract (decisions-log item 62) — a diff here after "just" swapping a heap is
  the contract telling you it noticed.
* **The cost constants moved.** `STEP_CARDINAL = 10`, `STEP_DIAGONAL = 14`,
  `CLIMB_SURCHARGE = 4` and `MOVE_COST_PER_TICK = 3` are rules-table data stamped
  into the rules hash (item 59), so a tuning change moves this file *and*
  `rules_hash` together. If only one moved, something is reading a constant it
  should be reading from the table.
* **Only the repaired routes moved.** The chunk-footprint repair is not
  equivalent to a from-scratch rebuild. T7's three repair tests are the ones to
  run first.
* **The estimate moved but the route did not.** Check the sign: item 61 makes
  *never optimistic* part of the contract, and the editor's rounding rests on
  it. An estimate that got smaller is the failure mode to look for.
