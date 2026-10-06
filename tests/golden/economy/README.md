<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `economy/` — the `$` and `kW` ledgers

**Filled by T14** (`crates/sim`).

Per-tick and per-event ledgers: mining and salvage credited on delivery, "paid
means yours" at commit, value following condition (build cost times current HP),
the brownout order, and BMI settled at each recap by rank on held value among
living seats.

## What a diff means

* **A tuning value moved.** Costs, draws, yields and BMI are rules-table data
  stamped into the rules hash (AGENTS.md §12), so this file and `rules_hash`
  move together. If only this moved, something is reading a constant in code
  that belongs in `rules/rules.v1.json`.
* **A rounding changed.** Kill-credit shares are apportioned as integers by
  largest remainder, ties to the lowest seat id. A one-unit diff spread across
  seats is usually the remainder rule, and it is hashed state like everything
  else.
* **Money appeared or vanished.** The treasury is single and the ledger should
  balance. An unexplained delta is a double credit or a charge that did not
  happen, not a golden to bless.
* **A grid rule changed, and no table value did.** Then `rules_hash` stays put
  and the pull request names the rule. T14b's is the one on record: a live
  beacon's base draw is netted out by its own key-core (decisions-log item 113
  (5)), so the `draw` column went from 6 to 4 on every row (the core's 2 kW base
  left it; the starting force's 4 kW did not) and nothing else in the file
  moved.
* **A mining rule changed, and no table value did.** S1's `mine` lane is the
  one on record (register S1-38): a drone digs the seam its home beacon holds
  from pit-safe stands rather than only the seam's exposed rim, so each core's
  starting drone, at the default `dig_max_depth` of 0, works its starting
  seam's whole top layer -- 38 voxels, `$` 152 at standard grade -- and the
  ledger delivers in rounds 2 and 3 where it had stopped after round 2 (the
  demo's F2). Every `ore_delivered` line, every closing column after it and
  the settlement ranks moved with it; `rules_hash` did not.
* **The brownout order changed.** Lowest priority, then furthest from the core,
  core last, autocannon never shed. A different shedding order is a gameplay
  rule change and needs to be stated as one.
