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

## The format

Tab-separated, one record per line, written by
`crates/sim/tests/pathing.rs`'s `the_path_hashes_match_their_golden`. Five kinds
of line, in this order:

```text
seed     <the match seed, hex>
cluster  <the cluster edge in voxels>
graph    <phase>  <graph fingerprint>  <transition nodes>  <directed abstract edges>
<phase>  <case>   <start>  <goal>  <cost>  <ticks>  <legs>  <walked cost>  <route nodes>  <route digest>
craters  <how many>  <columns edited>  store  <the chunk store's digest>
```

`<phase>` is `pristine` or `repaired`; `<start>` and `<goal>` are column ids
(`x + y * size_x`); `cost`, `ticks` and `legs` are what
`pharmakos_sim::pathing::estimate` returned at the commander's speed;
`walked cost` is the refined and smoothed route's own cost, which is never
dearer than the estimate; and the digest is xxh3 over the route's node sequence,
four little-endian bytes a node — the same form the state hash sees it in. A
query with no route writes `noroute` in place of the six numbers.

## What a diff means

* **The search changed.** Cost model, tie-break or heuristic. The ordering
  `(f, h, node_id)` in a hand-written binary min-heap is part of the determinism
  contract (decisions-log item 62) — a diff here after "just" swapping a heap is
  the contract telling you it noticed.
* **The cost constants moved.** `locomotion.step_cost_cardinal` = 10,
  `step_cost_diagonal` = 14 and `climb_surcharge` = 4 are rules-table data
  stamped into the rules hash (item 59), as are the per-kind
  `*_cost_per_second` speeds item 90 replaced `move_cost_per_tick` with — and
  the speed is what the `ticks` column is computed at. So a tuning change moves
  this file *and* `rules_hash` together. If only one moved, something is reading
  a constant it should be reading from the table.
* **Only the repaired routes moved.** The chunk-footprint repair is not
  equivalent to a from-scratch rebuild. T7's three repair tests are the ones to
  run first.
* **The estimate moved but the route did not.** Check the sign: item 61 makes
  *never optimistic* part of the contract, and the editor's rounding rests on
  it. An estimate that got smaller is the failure mode to look for.
