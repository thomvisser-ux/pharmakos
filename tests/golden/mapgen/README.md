<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `mapgen/` — per-seed map digests

**Filled by T5** (`crates/sim`: the seeded deterministic map generator).

One line per committed seed. The map is a pure function of `(match seed, rules
table, occupied seats)` drawn on `Stream::Map`, so these are byte-identical on
Windows, Linux and macOS or the generator is not deterministic.

## The format

`expected.digests.txt` is one line per seed, in ascending seed order, four
TAB-separated fields:

```text
0x<seed, 16 lowercase hex>  <store digest, 16 lowercase hex>  <raider s>  <commander s>
```

* **seed** — the match seed, which is also the map seed. Written `0x` + sixteen
  lowercase hex digits, always padded, so the file sorts as it reads.
* **store digest** — `VoxelStore::store_digest()`: xxh3-64 under the project
  seed over the map extent (three `u32`s), then the chunk count, then every
  chunk's own digest in chunk-index order. A digest of digests rather than of
  9.4 MB of bytes, for the reason decisions-log item 66 gives for the state hash
  itself.
* **raider s** — the closest two occupied spawn centres, in octile ground cost
  (10 / 14 per step), divided by `locomotion.raider_cost_per_second` and
  **rounded up**: seconds of raider walking.
* **commander s** — the same distance over `commander.cost_per_second`.

LF endings, a trailing newline, no carriage return. The producing test is
`the_per_seed_digests_match_their_golden` in `crates/sim/tests/mapgen.rs`, which
writes `<target>/golden/mapgen/actual.digests.txt`; `cargo xtask ci`'s `golden`
step compares the pair and `cargo xtask golden --bless` accepts a fresh one.

The seed set is eight seeds and **includes `0x00000000ca5caded`**, because the
committed scenarios name it: `expand-east-segment` and `deploy-and-visit` both
assert on a match played on that seed, so the seed belongs in the committed
set. (Until T15 the sentence here named a third,
`scenarios/skeleton/expand-east.scenario.jsonc`; T15 removed that file, because
it asserted a `beacon_placed` decisions-log item 103 (3) had already shown
cannot happen on this map. `deploy-and-visit` is what took its place, on the
same seed and with the same two seats.) The other seven span the space a `u64` seed can be wrong
in — zero and all-ones, one, the determinism harness's own seed, the project's
hash seed, the RNG's golden-ratio constant, and one arbitrary value.

The digests are generated at **three seats**, a full v1 match, so every zone the
map carries is occupied and every invariant the tests assert has something to
check.

**A seed alone does not name a map: `(seed, rules table, occupied seats)` does.**
An occupied zone stamps a core, a vent and a seam into the store and an
unoccupied one is terrain, so the same seed at two seats and at three is two
different stores with two different digests. Every line in this file is the
three-seat map. Both committed scenarios declare **two** seats, so their map is
*not* the line their seed appears on; what the golden pins for that seed is the
generator, not either scenario's own store.

T15 did not add a seat count to this file, and that is worth saying plainly
rather than leaving as a gap. A scenario's map is pinned already, by the thing
that depends on it: `tests/golden/scenarios/<name>/expected.hashes.txt` is a
per-tick chain over that exact store, so a two-seat map that generated
differently moves the chain at tick 0. A second golden carrying the same claim
in a weaker form would be one more file to re-bless every time the generator
moves, for a signal the chain already gives. **Owner decides** whether this
file should nonetheless carry `(seed, seats)` lines when a later stage wants a
map digest without a match behind it.

## What a diff means

* **The generator changed**, and every scenario and hash-chain golden built on
  these seeds moves with it. Expect a large blast radius; that is the point of
  committing the digest rather than the map.
* **An RNG stream or generation phase was reordered or reused.** Adding a stream
  or a `mapgen::Phase` with a fresh id is additive and safe; reordering the enum
  or reusing one for a second purpose is a determinism change (AGENTS.md §4.7)
  and moves every seed at once. A diff on *every* seed with no generator edit is
  this.
* **A rules-table row the generator reads changed.** The map extent, the spawn
  zones, the vent and seam bands, the contested counts and the richness grades
  are all `map.*`, and the two travel-time columns move with
  `locomotion.raider_cost_per_second`, `commander.cost_per_second`,
  `locomotion.step_cost_*` and `match.segment_lengths_ms[0]`. The rules table is
  an *input*, so this is a legitimate move — name the row in the pull request.
* **It moved on one platform.** Not a generation change — a determinism hole in
  the generator, which is as much a determinism artefact as the tick is.
* **A seed's map no longer satisfies its invariants.** The spawn-distance rule,
  vent reachability, ore parity across occupied zones and the power ceiling are
  asserted separately from the digest, by the four tests skeleton plan §3 (T5)
  names. Fix the invariant, then re-bless.
* **Only the last two columns moved.** The map is the same; the spawn-distance
  *report* is not. Either a locomotion row moved or the layout put two zones a
  little further apart. Check `min_separation_cost` against
  `required_separation_cost` in `MapReport` before blessing.
