<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `schema/` — generated schema and `get_schema`

**Filled by T13.** One case, `get_schema/expected.json`: the whole playbook
vocabulary as JSON Schema 2020-12, which is what `get_schema{}` with no `part`
answers. Produced by `crates/gateway/tests/methods.rs`, which compares the
**method's** answer rather than the generator's — so a gateway that started
assembling its own would move this file even if the generator had not changed.

The document is walked out of the checked-in descriptor set
(`crates/proto/src/generated/descriptor.binpb`) by
`crates/gateway/src/schema.rs`. Nothing in it is written down twice, which is
the whole of what "so they can't drift" buys.

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

One thing this file cannot catch, and so does not have to: **a `$defs` key
written twice.** The keys are last segments of full names (`Step`, `Meta`) and
`gp.v1` already declares two enums called `Kind`; if both were ever reachable
from one root, one definition would overwrite the other and every `$ref` to it
would name the wrong type — with this golden agreeing, because this golden *is*
the output. `crates/gateway/src/schema.rs` therefore refuses a second claimant
on a key rather than overwriting, so that day is a failed build and not a
blessed diff.
