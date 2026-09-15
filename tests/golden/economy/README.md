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
* **The brownout order changed.** Lowest priority, then furthest from the core,
  core last, autocannon never shed. A different shedding order is a gameplay
  rule change and needs to be stated as one.
