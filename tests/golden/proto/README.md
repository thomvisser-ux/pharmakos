<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

<!--
This directory is the one golden area in the permissive tier, because its
contents are the same public surface as `proto/**`: canonical `gp.v1` JSON and
views of the schema, which an alternative playbook tool has to be able to read
as the reference they are. REUSE.toml carves `tests/golden/proto/**` out of the
`tests/**` block for exactly that reason, so this README takes the permissive
header its directory takes rather than the GPL header the other areas take.
-->

# `proto/` — `gp.v1` canonical JSON

**Filled by T1** (`crates/proto`) — landed. `crates/proto/tests/proto.rs` writes
the fresh `actual.*` files into `<target>/golden/proto/` and the `golden` step
byte-compares them with the four `expected.*` files here.

Round trips through the canonical proto3-JSON codec: a committed playbook
decodes with **zero unknown fields**, re-encodes to bare-number durations, and
the result is pinned here (`expected.expand_east.json` and its siblings).

## What a diff means

* **The wire format changed**, which is an AGENTS.md §5 contract change. Fields
  are added, never renumbered or reused; reserved numbers stay reserved, except
  that a number held for a named future row is *discharged* by that row (a
  range such as `RulesTable.Economy`'s narrows by the number the row takes,
  under a scoped `ignore_only` in `proto/buf.yaml`; decisions-log items
  100 (10), 109 and 111);
  `buf breaking` runs in `WIRE_JSON` mode in CI and should have caught it first.
  If `buf breaking` is green and this moved, the *mapping* changed rather than
  the schema — which is the harder bug, because nothing else guards it.
* **A duration is no longer a bare number.** Every duration in `gp.v1` is
  `int32` game milliseconds (item 46). A golden that grew quotes or a unit
  suffix is the codec drifting from the disk format the whole project rests on.
* **An unknown field survived.** Submitted playbooks with unknown fields are
  *rejected*, never stripped (AGENTS.md §5, §11). A golden that gained a field
  nobody declared means the decoder is tolerant where it must not be.

## What a diff in each file means

* **`expected.reserved.txt`**: a reservation was added, or dropped. Added is
  safe. Dropped is a `WIRE_JSON` break unless it is a held number discharged by
  the row it was held for, as above; S1's first contract pull request
  discharged `RulesTable.Economy`'s 12 for `economy.mining_carry_voxels`, so
  the range reads 13-15.
* **`expected.rules.v1.json`**: the rules table's canonical JSON, so a row was
  added or a value re-tuned. The table is hashed into `rules_hash`, so the same
  pull request re-blesses every verifier `report_hash` golden, the gateway's
  `demo_*_verify` cases and the sim's pinned `rules_hash`
  (`crates/sim/tests/determinism.rs`), and says which rows moved and why. A
  row the sim reads moves the hash chains as well.
* **`expected.method-scopes.txt`**: a gateway method was added or its
  `required_scope` annotation changed, which is a `gp.api.v1` contract change.
* **`expected.expand_east.json`**: the canonical codec's output for a committed
  playbook, as the section above describes.
