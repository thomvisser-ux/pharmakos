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
  are added, never renumbered or reused; reserved numbers stay reserved;
  `buf breaking` runs in `WIRE_JSON` mode in CI and should have caught it first.
  If `buf breaking` is green and this moved, the *mapping* changed rather than
  the schema — which is the harder bug, because nothing else guards it.
* **A duration is no longer a bare number.** Every duration in `gp.v1` is
  `int32` game milliseconds (item 46). A golden that grew quotes or a unit
  suffix is the codec drifting from the disk format the whole project rests on.
* **An unknown field survived.** Submitted playbooks with unknown fields are
  *rejected*, never stripped (AGENTS.md §5, §11). A golden that gained a field
  nobody declared means the decoder is tolerant where it must not be.
