<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `schema/` — generated schema and `get_schema`

**Filled by T1** (the generated JSON Schema) and T13 (`get_schema`).

Regenerated and compared on every run, so the published schema cannot drift from
the `.proto` files it comes from (AGENTS.md §9 item 8).

## What a diff means

**The schema drifted, or you changed it.** There is no third reading.

* **You changed `proto/`.** Then this diff is the regeneration, and the same
  pull request carries both. `buf lint` and `buf breaking` in `WIRE_JSON` mode
  have already had their say about whether the change is allowed.
* **You changed nothing under `proto/`.** Then the generator, its version or its
  configuration moved, and the published surface changed without a schema
  change. That is the case this golden exists to catch; pin the generator rather
  than blessing the output.
* **`get_schema`'s output moved but the JSON Schema did not.** The gateway is
  building its answer rather than serving the generated artefact. Spec §12 asks
  for generated, exactly so it cannot drift.
