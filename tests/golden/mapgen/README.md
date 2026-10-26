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

### `expected.features.txt` — the feature table (S1)

**Added by S1's targeting** (the `tgt` lane; `docs/design/targeting.md`,
"Names"; decisions-log item 127 (12)), a new golden in this area and so a
golden-format change. One line per vent and per seam of each seed's three-seat
map, seeds in the order of `expected.digests.txt`, and within a seed in the
feature table's own `(anchor y, anchor x)` order — the resolver's tie-break.
Five TAB-separated fields:

```text
0x<seed, 16 lowercase hex>  <name>  <grade>  <footprint columns>  <anchor top z>
```

* **name** — the feature's name as a playbook writes it: `vent_<x>_<y>` or
  `seam_<x>_<y>`, from the generation anchor column (the patch or disc centre),
  with no z because craters change z.
* **grade** — `lean`, `standard` or `rich`.
* **footprint columns** — how many columns the generator stamped it into: nine
  for a vent's patch, about forty for a seam's disc.
* **anchor top z** — the anchor column's top solid voxel when it was stamped,
  the voxel the feature is made of.

The producing test is `the_per_seed_features_match_their_golden`, which writes
`<target>/golden/mapgen/actual.features.txt`. The table is a pure function of
`(seed, rules table, occupied seats)`, regenerated on a restore and never
hashed per tick, so it moves with the generator and never with play.

**What a diff here means.** A moved **name** is a moved anchor, and a name is
what a sealed playbook carries: a carried `vent_120_88` that no longer names a
vent fails its step `no_target`. Expect it only with a generator change, which
moves `expected.digests.txt` on the same seed; a features diff with the digests
unchanged means the table's bookkeeping changed (which columns count as a
feature's footprint), which is the rules a liveness scan and "one feature per
column" read, and is explained in the pull request that moved it. The
generator refuses a map whose features share an anchor or a footprint column
(`MapError::FeatureOverlap`), so a seed that stops generating is that check.

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
moves, for a signal the chain already gives. **PLACEHOLDER: whether this file
should nonetheless carry `(seed, seats)` lines. OWNER, when a later stage wants
a map digest without a match behind it.** (Spelled with the marker because a
review found a tree-wide `grep PLACEHOLDER` missing this one — AGENTS.md §12
fixes the spelling.)

### `expected.bore.txt` — the core's bore (S2)

**Added by S2's `fire`** (S2's plan, E31; decisions-log item 144 (1)), a new
golden in this area and so a golden-format change. The bore is the core
anchor's footprint column, from the bedrock layer to the generated surface,
which no blast cracks or removes: a table **derived** from the seed, the rules
and the occupied seats, never hashed per tick and never carried in a snapshot,
so it is pinned here as the feature table is (AGENTS.md §4.8's derived-table
rule). One line per occupied seat of each seed's three-seat map, seeds in the
order of `expected.digests.txt`, seats ascending. Six TAB-separated fields:

```text
0x<seed, 16 lowercase hex>  <seat>  <x>  <y>  <lowest z>  <highest z>
```

* **seat** — the seat whose core stands on the column.
* **x**, **y** — the core's column, in whole voxels.
* **lowest z** — always `0`: the bore runs down through the bedrock layer.
* **highest z** — the column's top solid voxel at generation, the surface the
  core stands on.

The producing test is `the_bore_is_derived_and_pinned` in
`crates/sim/tests/mapgen.rs`, which writes `<target>/golden/mapgen/actual.bore.txt`.
A moved line is a moved core: a generator change, which moves
`expected.digests.txt` too.

**Bedrock** (S2's `fire`, E14) changed the bottom `materials.bedrock_layers_voxels`
layers of every column from stone to bedrock, so every seed's store digest in
`expected.digests.txt` moved once for it, and nothing else about the maps did.

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

### `expected.surface.txt` — the generated surface (S2)

**Added by S2's `site`** (decisions-log item 144 (5); S2's plan, task
`site`), a new golden in this area and so a golden-format change. Terraform's
`FILL` and `BOTH` refill craters and trenches inside a Build beacon's sphere
"back to the generated surface": each column's top solid voxel as the
generator left it. The table is **derived** from the seed, the rules and the
occupied seats, regenerated by `Snapshot::restore_into`, never hashed per tick
and never carried in a snapshot, so it is pinned here as the bore and the
feature table are (AGENTS.md §4.8's derived-table rule). One line per seed of
the three-seat map, in the order of `expected.digests.txt`. Five TAB-separated
fields:

```text
0x<seed, 16 lowercase hex>  <surface digest, 16 lowercase hex>  <columns>  <lowest top>  <highest top>
```

* **surface digest** — `GeneratedSurface::digest()`: xxh3-64 under the
  project seed over the map's two horizontal extents and then every column's
  top, in `(y, x)` order, each as a little-endian `i32`.
* **columns** — how many columns the table holds: the map's width times its
  depth.
* **lowest top**, **highest top** — the lowest and highest top solid voxel
  over the map, so a diff says at a glance whether the terrain's range moved.

The producing test is `the_generated_surface_is_derived_and_pinned` in
`crates/sim/tests/mapgen.rs`, which writes
`<target>/golden/mapgen/actual.surface.txt`; it also holds a sample of
columns to the store's own tops. A moved line is a moved generator, which
moves `expected.digests.txt` on the same seed too.
