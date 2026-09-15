<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `proto/` — `gp.v1` canonical JSON

**Filled by T1** (`crates/proto`).

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
