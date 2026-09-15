<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `mapgen/` — per-seed map digests

**Filled by T5** (`crates/sim`: the seeded deterministic map generator).

One digest per committed seed. The map is a pure function of the seed on
`Stream::Map`, so these are byte-identical on Windows, Linux and macOS or the
generator is not deterministic.

## What a diff means

* **The generator changed**, and every scenario and hash-chain golden built on
  these seeds moves with it. Expect a large blast radius; that is the point of
  committing the digest rather than the map.
* **An RNG stream was reordered or reused.** Adding a stream is additive and
  safe; reordering the enum or reusing a stream for a second purpose is a
  determinism change (AGENTS.md §4.7) and moves every seed at once. A diff on
  *every* seed with no generator edit is this.
* **It moved on one platform.** Not a generation change — a determinism hole in
  the generator, which is as much a determinism artefact as the tick is.
* **A seed's map no longer satisfies its invariants.** The spawn-distance rule,
  vent reachability, ore parity across occupied zones and the power ceiling are
  asserted separately from the digest. Fix the invariant, then re-bless.
