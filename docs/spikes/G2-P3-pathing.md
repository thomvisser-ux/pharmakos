<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# G2 + P3 — Pathing and travel estimates (spike, wk 1–3)

## 1. Goal and go/no-go criterion

**Goal.** Prove that our own HPA\* — section 15's crate row says *"pathing primitives with our own HPA\*"* and explicitly says to avoid `hierarchical_pathfinding` — can repath hundreds of walkers over terrain that is being edited underneath them, and can produce a travel-time estimate accurate enough for the editor to show a number rather than a shrug.

This gate matters twice over. The performance half (G2) is ordinary. The accuracy half (P3) is a **design** dependency: the whole game is "orders you cannot take back", and a playbook's conditions lean on the clock. If the estimate cannot be trusted to ±15%, the editor must stop promising an ETA and the playbook vocabulary has to lean on arrival triggers instead — a change to the editor and to the language, which is why it must be known before the skeleton fixes them.

**Go/no-go criterion, copied verbatim from the section 16 gate table:**

> **G2 + P3 Pathing and travel estimates** — HPA\* prototype in the spike, wk 1–3; certified in the sim at S3 — Go if: *"Repath p99 ≤5 ms; travel estimate ≤1 ms and within ±15% (p90) on static terrain"* — Fallback: *"Show ranges; favour arrival triggers"*

| # | Assertion | Pass condition |
|---|---|---|
| G2-a | Repath latency | p99 of a single unit's repath ≤ **5 ms** |
| P3-a | Estimate latency | a travel estimate query ≤ **1 ms** |
| P3-b | Estimate accuracy | estimate within **±15% at p90** of actual travel, **on static terrain** |

Note what the spike is and is not. Section 16 puts the *prototype* here in wks 1–3 and the *certification* in the sim at S3, and section 17 lists S3's depth task as *"HPA\* pathing in the sim and the travel estimator (G2+P3, ±15%)"*. So this spike's job is to establish that the numbers are reachable and to fix the design choices (cluster size, cost model, estimator shape) before the skeleton commits to them. The binding measurement happens later, on real code.

## 2. The toy build

`spikes/g2-pathing/` — a standalone Rust crate, no Godot, no rendering. Standalone workspace; thrown away.

```
spikes/g2-pathing/
  Cargo.toml            # [workspace] (empty); overflow-checks = true in release
  src/
    world.rs            # 384x384x64 voxel grid; walkability derivation
    cost.rs             # integer step costs; no floats anywhere
    astar.rs            # low-level A* with a deterministic total-order tiebreak
    clusters.rs         # HPA* cluster decomposition, entrances, abstract graph
    hpa.rs              # abstract search + per-leg refinement + path smoothing
    repair.rs           # incremental rebuild of the clusters an edit dirtied
    estimate.rs         # travel-time estimate (the P3 query), with the fog x1.5 rule
  src/bin/
    bench.rs            # the measurement harness: wall clock lives HERE, not in the lib
    accuracy.rs         # estimate vs ground truth vs simulated walk
```

**The world model** follows section 9's locomotion rules, and nothing more:

- *"Everything walks… its own fixed speed, climbing steps of one voxel. Nothing flies in v1. Walls, craters and trenches block every unit."* So the walkable set is: a voxel column's surface cell is walkable if it is solid, the cell above is empty with enough headroom, and the step up or down to a neighbour is at most 1 voxel. That makes the search graph essentially a **2.5D surface graph** over a 384×384 footprint with a height per cell — about 147k nodes, not 9.4M — which is a large part of why this is tractable. Record the actual node count; a map with overhangs or bridges would need more than one node per column, and the spike must say whether the generator can produce those.
- **8-connected** movement on the surface. Costs are integers: 10 for a cardinal step, 14 for a diagonal (the standard integer octile approximation), plus an integer surcharge for a 1-voxel climb. No floats — the lint set forbids them and the heuristic must stay admissible under integer scaling. Record the surcharge as a tuning value.
- **Terrain edits during the Push** are the interesting part: *"Pathing handles terrain edited during the Push."* The bench replays a destruction stream (the same explosion generator shape as G1, at 20 explosions/s) that invalidates surface cells, and forces the affected clusters to be repaired and the units crossing them to repath.

**HPA\* as prototyped**: clusters of **32×32 cells** so a cluster's footprint matches a chunk's, which makes "an explosion dirtied chunk (cx, cy)" map straight onto "repair cluster (cx, cy)". 12×12 = 144 clusters. Entrances are maximal runs of mutually-walkable border cells between adjacent clusters, one transition node per run (two for long runs). The abstract graph holds intra-cluster edges precomputed by a bounded A\* between each pair of a cluster's transition nodes. A query inserts start and goal as temporary nodes, searches the abstract graph, then refines each leg with a cluster-local A\*. One abstract level to begin with; the spike measures whether a second level earns its keep at this map size (at 144 clusters, it probably does not — record the evidence rather than the intuition).

**The estimator** is the same machinery with the refinement skipped: search the abstract graph, sum the integer edge costs, convert to ticks at the unit's fixed speed. That is the whole reason the estimate can be ≤1 ms while a path is ≤5 ms. Section 11's allowed-estimates table is the constraint: *"Pathfinder travel estimates over known terrain; fogged voxels use last-known state or ×1.5 cost"*, and the forbidden column rules out stepping or forking the sim — so the estimator may never simulate a walk. It computes a cost and divides.

## 3. Measurement procedure

**Step 0 — Prerequisites.** Rust toolchain (MSVC ABI on Windows). Pin the version. No other dependencies: the spike writes its own A\*, its own heap and its own cluster code, because that is what the crate row commits to.

**Step 1 — Build a test map.** Not random noise: a deterministic generator in the spirit of section 9 — 3-way rotational symmetry, ridges, a few chokepoints, ore seams, one big central basin. Random noise makes pathing look easy and estimates look accurate; chokepoints and long detours are where an abstract estimate goes wrong, and they are what the real generator produces. Record the map seed and the walkable-cell count.

**Step 2 — Ground truth.** Implement a plain low-level A\* over the same integer cost model, with a consistent heuristic, and an exhaustive Dijkstra from a set of sources. Verify the A\* against Dijkstra on 1,000 pairs: costs must match exactly, or the heuristic is not admissible and every accuracy number afterwards is garbage. Record mismatches (must be zero).

**Step 3 — Path quality.** For 2,000 deterministic start/goal pairs stratified by straight-line distance into bands (≤32, 32–96, 96–256, >256 cells), compare HPA\* path cost against optimal (low-level A\*) cost. Record **excess cost** as a percentage: p50 / p90 / p99 / max. HPA\* is not optimal by construction; the spike needs to know by how much, because the estimate inherits this error *and adds its own*.

**Step 4 — Repath latency (G2-a).** Simulate 300 units on active routes (the G3′ unit count, used here so the two spikes speak the same language). Run the destruction stream. Every time an edit invalidates a cell on a unit's remaining path, that unit repaths. Record, over ≥20,000 repaths:

- repath time: p50 / p90 / **p99** / max, in ms — **p99 must be ≤5 ms**;
- repaths per second at the peak of the destruction stream;
- cluster repair time per dirtied cluster: p50 / p99 / max;
- abstract graph size (nodes, edges) and its memory;
- per-query scratch memory and allocations;
- whether any repath fails to find a path, and what the caller does then (a walker sealed behind its own moat is a legal game state — section 9 says "a moat seals you in as well" — so "no path" must be a normal, cheap result, not a pathological search).

Record repath latency **both** with a cold cache (first repath after a repair) and warm; the p99 that matters is the cold one, since it coincides with the destruction that caused it.

**Step 5 — Estimate latency (P3-a).** 10,000 estimate queries at the same distance stratification. Record p50 / p90 / **p99** / max in ms — **must be ≤1 ms**. Also record the cost with a cold abstract graph immediately after a repair, since the editor's estimate is requested during the Lull right after a Push has rearranged the terrain.

**Step 6 — Estimate accuracy (P3-b).** This is the number that decides the editor's vocabulary, so measure it against the *right* baseline. For each of 2,000 pairs on **static terrain**, record three quantities:

1. `estimate_ticks` — what the estimator returns;
2. `optimal_ticks` — cost of the optimal low-level path, converted at unit speed;
3. `walked_ticks` — the actual tick count when a unit is stepped along the refined HPA\* path by the toy's movement integrator, which is what a player actually experiences.

Report relative error of `estimate` against `walked` (the honest comparison) and against `optimal` (the diagnostic one): p50 / **p90** / p99 / max, signed, so systematic over- or under-estimation is visible. **p90 of |error| must be ≤15%.** A systematic bias is easier to fix than variance — if the estimator is consistently 8% low, a calibration constant fixes it and the spike should say so.

**Step 7 — Fog, recorded separately.** Repeat step 6 with a fog mask over part of the route and the ×1.5 cost rule applied to unknown cells. The gate says "on static terrain", so this does not decide the gate — but the editor will show these estimates to players, and section 13 says fogged legs draw dashed. Record the fogged error distribution so S3 knows what it is dealing with.

**Step 8 — Dynamic terrain, recorded separately.** Estimate accuracy while the destruction stream runs. Also not part of the gate — but it is the condition players will actually be in, so record the degradation.

**Step 9 — Determinism.** Run the whole bench twice on the same machine and once on the other OS, and confirm every path is identical (hash the path node sequences). Pathing output is hashed sim state; a platform-dependent path would break G4. If paths differ, the tiebreak is not total — fix it here, where it is cheap.

**Step 10 — Write up.**

### Numbers to record (the results table)

| Number | Unit | Gate part |
|---|---|---|
| Walkable cells, abstract nodes, abstract edges | count | scale |
| HPA\* excess cost vs optimal, p50/p90/p99 | % | feeds the estimate error |
| Repath time p50/p90/**p99**/max (cold / warm) | ms | **G2-a: p99 ≤ 5** |
| Cluster repair time p50/p99/max | ms | churn cost |
| Repaths/s sustained at peak destruction | /s | headroom |
| Estimate query p50/p90/**p99**/max | ms | **P3-a: ≤ 1** |
| Estimate vs walked, signed error p50/**p90**/p99/max | % | **P3-b: p90 ≤ ±15** |
| Estimate vs optimal, signed error | % | diagnosis |
| Fogged-route error (×1.5 rule) | % | information for S3 |
| Dynamic-terrain error | % | information for S3 |
| Memory: graph, per-query scratch, peak RSS | MiB | fits alongside sim + mesher |
| Path identity across runs and OSes | yes/no | protects G4 |
| Cluster size sweep: 16 / 32 / 64 | all of the above | the cluster-size decision |

## 4. CI hook

`.github/workflows/spike-g2.yml`. Modest, because this spike's binding measurement is at S3:

```yaml
name: spike-g2-pathing
on: [push, workflow_dispatch]
jobs:
  bench:
    strategy: { matrix: { os: [ubuntu-latest, windows-latest] } }
    runs-on: ${{ matrix.os }}
    steps:
      - run: cargo test --release          # A* vs Dijkstra exactness; tiebreak totality
      - run: cargo run --release --bin bench -- --units 300 --json bench-${{ matrix.os }}.json
      # assert repath p99 <= 5 ms and estimate p99 <= 1 ms, with a PLACEHOLDER margin
      # that accounts for hosted-runner noise (hosted vCPUs are slow and noisy; the
      # binding numbers come from the dev machine, CI guards against regression).
      - run: cargo run --release --bin accuracy -- --pairs 2000 --json acc-${{ matrix.os }}.json
      # assert |error| p90 <= 15% against walked time on static terrain
      - run: cargo run --release --bin bench -- --hash-paths --out paths-${{ matrix.os }}.txt
  path-identity:
    needs: bench
    runs-on: ubuntu-latest
    steps:
      - run: cmp paths-ubuntu-latest/* paths-windows-latest/*   # paths are hashed sim state
```

The path-identity job is the one worth keeping past the spike: it is a cheap, early instance of the cross-OS determinism contract G4 is establishing, applied to the subsystem most likely to break it.

The sketch above is a sketch; the file that runs is `spikes/g2-pathing/ci/spike-g2.yml`, and two things about it are worth carrying forward when the job is promoted. A workflow-level `defaults.run.working-directory` reaches into *every* job, so the `path-identity` job — which checks nothing out — has to override it back to the workspace root or its first `run` step fails before executing a command. And `actions/download-artifact`'s `path:` resolves against `$GITHUB_WORKSPACE`, not against that working directory, so the two have to agree.

## 5. Platform risks: Windows/MSVC vs Linux

| Risk | Why it differs | How this spike handles it |
|---|---|---|
| **Priority-queue tiebreak** | `BinaryHeap` gives no order among equal keys, and the order it *does* produce depends on insertion history and on the std implementation. Two platforms with different toolchain builds can pop equal-cost nodes in different orders and return different — equally optimal — paths. Since paths are hashed sim state, that breaks G4. | The `Ord` on a search node is a **total order**: `(f_cost, h_cost, node_id)`. Never compare on cost alone. Step 9 asserts path identity across OSes; a failure there is a tiebreak bug, not a platform bug. |
| **Sort stability** | Same argument, wherever entrances or neighbours are sorted. | Every sort key ends in a unique id. |
| **Iteration order** | `HashMap`/`HashSet` for open/closed sets is the default habit in every A\* tutorial, and it is forbidden by the lint set and non-deterministic by design. | Open set is the heap; closed set and g-scores are **dense arrays indexed by node id** (fastest and inherently ordered). Sparse per-query state uses a generation-stamped array rather than a map, so clearing is O(1) and order-free. |
| **Integer overflow** | Costs accumulate across up to a few thousand steps at 10–14 per step plus climb surcharges; a `u16` or `i16` cost would overflow on a long path, and overflow checks (on in every profile) would panic on one platform under one seed. | Costs are `i32` with a documented maximum (`cells × max_step_cost` computed and asserted at startup); the heuristic uses the same width. Tests include the longest possible path on the map. |
| **`usize` and pointer width** | Node ids as `usize` leak target width into anything hashed or serialised. | Node ids are `u32`. 147k nodes leaves three orders of magnitude of headroom. |
| **Stack size** | MSVC threads default to ~1 MB of stack against Linux's ~8 MB. A recursive path-refinement or flood-fill overflows on Windows only, often only on the biggest map. | No recursion anywhere: cluster refinement, entrance discovery and connectivity flood-fill all use explicit `Vec` work stacks. Tested with a worst-case single-region map. |
| **Wall-clock time in the library** | The lint set forbids wall-clock time in sim code, and pathing is sim code — but the whole spike is a timing measurement. | `Instant` appears only in `src/bin/bench.rs` and `accuracy.rs`. The library never reads a clock. This is the same wall the product will need for its own benches, so the spike is prototyping the arrangement. |
| **`as` casts** | Forbidden by the lint set, and cast truncation between cell coordinates and node ids is a classic silent-wrong-path bug. | `TryFrom` with explicit handling; coordinate↔id conversion is one audited function pair with round-trip tests. |
| **Allocator behaviour** | Per-query allocation is the easiest way to make a 0.5 ms query into a 5 ms one, and the Windows default allocator punishes it harder than glibc. | A per-unit scratch pool reused across queries; allocations per query are recorded (target: zero in the steady state). If Windows still trails, record the gap rather than papering over it. |
| **Timing noise on shared runners** | Hosted CI vCPUs are slow and noisy; a p99 measured there is not the p99 the game has. | Binding numbers come from the dev machine on both OSes; CI asserts a looser `PLACEHOLDER` threshold purely as a regression alarm, and the threshold is written down as such. |
| **Floats** | None. The cost model, the heuristic and the tick conversion are integer throughout — the octile 10/14 approximation exists precisely so no square root and no float is needed, and section 15 already rules out `cordic`. | The only division is cost→ticks at unit speed, done in integer with an explicit rounding rule that is stated and tested. |

## 6. Fallback if it fails

The spec's fallback is **"Show ranges; favour arrival triggers"**. Expanded:

- **The editor stops showing an ETA it cannot keep.** Section 13 says routes draw as polylines with travel estimates and fogged legs draw dashed. Under the fallback, a route shows a **reachable-range band** — "3–5 minutes", or a reachable-by-time isochrone on the map — rather than a single number. A band is honest about the uncertainty the measurement found, and it is cheap to compute from the same abstract graph with a deliberately wide margin.
- **The playbook vocabulary leans on arrival triggers.** Conditions phrased against the clock ("at 4:00, launch") give way to conditions phrased against events ("when the group arrives at the staging point, launch"). Section 5's Attack mandate already has both — *"launch when any of force size, segment time or a broadcast go-code holds"* — so the fallback shifts emphasis rather than adding machinery: templates, the wizard and the docs favour the arrival form, and the editor's guided first pick uses it.
- **Which sub-assertion failed changes the response.** If only **P3-b** (accuracy) fails, take the fallback above and keep the pathing as it is. If only **G2-a** (repath latency) fails, the response is not the editor's — it is the sim's: coarsen the cluster size, cap repaths per tick with a deterministic round-robin queue, or let units continue on a stale path for a bounded number of ticks before repathing. If **P3-a** (estimate latency) alone fails, cache estimates per (start-cluster, goal-cluster) pair and invalidate on repair; an estimate is a Lull-time query, so a cache is very effective.
- **What does not change.** The gate cannot stop the project (section 16), and S3 still certifies pathing on real code. A spike failure here means S3 starts from the fallback design rather than discovering the problem at week 34.

## 7. Estimate

**4 days**, in worktree A during week 2 of the 3-week budget.

| Day | Work |
|---|---|
| 1 | World + walkability + integer cost model; low-level A\* and Dijkstra; exactness test (step 2) |
| 2 | Clusters, entrances, abstract graph, refinement; path-quality numbers (step 3) |
| 3 | Incremental repair under the destruction stream; repath and estimate latency (steps 4–5) |
| 4 | Accuracy harness with simulated walk (step 6); fog and dynamic runs; cluster-size sweep; cross-OS path identity; write-up |

Owner review once, at the end of day 4, on the estimator's contract — because the editor and the playbook vocabulary depend on whether it returns a number or a band.

## 8. The decision this spike must produce

> **Does the editor promise a travel time, or a range?** — and the pathing design that answer rests on: (1) cluster size and how many abstract levels; (2) the integer cost model — cardinal/diagonal weights, the 1-voxel climb surcharge, and the rounding rule for cost→ticks; (3) the incremental repair strategy and its granularity (cluster = chunk footprint, or not); (4) the estimator's API and error contract — what it returns, what it costs, how fog is priced, and what the editor is allowed to render from it; (5) the total-order tiebreak that keeps paths identical across platforms.

Recorded in `docs/design/decisions-log.md` §2.7. Item (5) is part of the determinism contract and needs owner approval alongside G4's.

## 9. Results

**Verdict: go, at cluster 16 or 32; recommended 32.** Every gate assertion passes
at cluster 16 and 32, and passes with enough room that the fallback in §6 is not
needed in any part: the editor may promise a travel **time**, not a range — with
two caveats the editor must honour (short routes, and fog). At cluster 64 the
repath gate still passes but **P3-a fails outright**, which is what makes the
cluster-size decision a measurement rather than a preference.

| Assertion | Budget | 16 | 32 | 64 |
|---|---|---|---|---|
| G2-a repath p99, cold | ≤ 5 ms | 1.08 ms ✅ | **1.84 ms ✅** | 3.65 ms ✅ |
| P3-a estimate p99 | ≤ 1 ms | 0.97 ms ⚠️ | **0.82 ms ✅** | 1.43 ms ❌ |
| P3-b \|error\| p90 vs walked, static | ≤ 15 % | 3.8 % ✅ | **2.6 % ✅** | 1.6 % ✅ |

Cluster 16's ⚠️ is a pass on the aggregate and a fail on the population that
matters: see §9.5 — its estimate p99 *within the >256-cell band alone* is
1.08 ms, over the budget. The aggregate only passes because the stratification
mixes in three bands of short routes. That, and not the 3% aggregate headroom, is
the reason to prefer 32.

### 9.1 Provenance

- **Machine:** Intel Core i7-9800X, 8C/16T @ 3.79 GHz · 31.7 GiB RAM · Windows
  10 Pro 19045 (10.0.19045). Nothing else heavy running; the spike directory was
  **not** excluded from Windows Defender, so the numbers include whatever
  real-time scanning costs on this machine.
- **Toolchain:** Rust 1.98.1 stable-x86_64-pc-windows-msvc (rustc `48a229cea`,
  2026-09-01), pinned by `spikes/g2-pathing/rust-toolchain.toml`. Release
  profile: `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`,
  **`overflow-checks = true`** (on in every profile, AGENTS.md §4.1). No
  dependencies at all — own A\*, own binary heap, own cluster code, own JSON
  writer, own FNV-1a-64.
- **Measured** 2026-09-14. Raw artefacts in `spikes/g2-pathing/results/`, which
  is git-ignored on purpose (`spikes/README.md`): what survives them is this
  section.
- **Map:** seed 20 260 913, 384 × 384 × 64 voxels, 3-fold rotationally symmetric
  height field, generated in 16–29 ms.
- **Reproduce** (from `spikes/g2-pathing`, toolchain on `PATH`):

  ```sh
  cargo test --release                     # 42 tests: step 2 exactness, tiebreak totality, repair identity
  cargo clippy --all-targets --release -- -D warnings
  # steps 1-8, once per cluster size; cluster 32 three times for the spread
  cargo run --release --bin bench    -- --units 300 --cluster 32 --json results/bench-c32-run1.json
  cargo run --release --bin accuracy -- --pairs 2000 --cluster 32 --json results/accuracy-c32.json
  # step 9, same machine: two runs, compared byte for byte
  cargo run --release --bin bench -- --hash-paths --cluster 32 --out results/paths-c32-a.txt
  cargo run --release --bin bench -- --hash-paths --cluster 32 --out results/paths-c32-b.txt
  cmp results/paths-c32-a.txt results/paths-c32-b.txt
  ```

  Peak working set is read from **outside** the process, because the harness may
  not take the dependency that would let it read its own (§9.9 item 1):

  ```powershell
  $p = Start-Process -FilePath .\bench.exe -ArgumentList '--units','300','--cluster','32' -PassThru -NoNewWindow
  $peak = 0
  while (-not $p.HasExited) { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 }; Start-Sleep -Milliseconds 40 }
  ```

- **What was run:** 9 whole-bench runs (3 at cluster 32, 2 each at 16 and 64 —
  one of each pair under the PowerShell sampler, used only for the memory row),
  3 accuracy runs (one per cluster size), and 4 `--hash-paths` runs. Each bench
  run is ~28 s at cluster 32 and drives ≥20 000 repaths; each accuracy run is
  ~19 s over 2 000 pairs plus 1 000 exactness pairs.
- **Every timing figure below is quoted at the precision the spread supports.**
  Three cluster-32 runs of the identical binary and seed put the headline p99
  between 1.799 and 1.840 ms, so it is "about 1.8 ms", never six decimals
  (§9.8). Percentiles in the tables are the **median across runs** where more
  than one run exists, and the table says so.
- **`cargo test --release` is green: 42 tests, 0 failures.** `cargo clippy
  --all-targets --release -- -D warnings` is clean under the spike-local
  `clippy.toml`, whose header documents the arrangement: the library is sim code
  and stays under the product's determinism rules (no floats, no `as`, no
  `HashMap`/`HashSet`, no wall clock), with `std::time::Instant` confined to
  `src/bin/*.rs` and that confinement checked by a test (`lib.rs`'s `mod wall`),
  not by a promise.

### 9.2 Step 1 — the map, and the walkable-cell count that is not a scale number

| | value |
|---|---|
| Columns (`NODES`) | 147 456 (384 × 384) |
| **Walkable cells** | **147 456 — 100%, by construction** |
| **Directed legal steps** (the real graph size) | **990 678** on the pristine map |
| Largest component / components, pristine | 130 937 / 4 234 |
| Ore cells | 3 678 |
| Overhangs, bridges, tunnels possible? | **No** — one surface node per column |
| `MAX_PATH_COST` bound, asserted at start-up | 2 654 208 (fits `i32` with 810× headroom) |

Two things a reader should take from this and the plan did not anticipate.

**"147 456 walkable cells" is not a scale number.** `World::generate` clamps
every column height into `1..=WY-6`, so every column passes the walkability test
by construction and the count is 100% of the grid. The **headroom** half of §2's
walkability rule (`HEADROOM = 2` empty voxels above the surface) is therefore
never exercised by this generator, and `assert_cost_headroom(walkable)` re-derives
the cost bound rather than testing anything about the map. What actually blocks
movement here is the **one-voxel step rule**, which is why every table below
leads with *directed steps* and component sizes: craters knock only 71 of 43 226
edited columns down to "no surface", but they destroy 131 356 directed steps and
shatter the largest component from 130 937 cells to 35 461.

**The generator cannot produce overhangs or bridges, and this is structural, not
a seed accident.** `OVERHANGS_POSSIBLE` is a `const false`, checked by a test:
`World` stores one `top` height per column, so there is nowhere for a second
surface to live. The spike therefore measures a 2.5D surface graph with branching
factor 8 and exactly one node per column — about 147k nodes rather than 9.4M,
which is a large part of why the numbers are what they are. What a generator with
overhangs would change is recorded in `world.rs`'s module docs and is smaller
than it sounds: the id↔coordinate bijection becomes a per-column surface list
plus an offset table, the neighbour function has to choose *which* surface of the
neighbouring column it steps to, and the node count stops being a compile-time
constant so the dense search arrays are sized at load. The cost model, the
heuristic, the cluster decomposition and the repair strategy see only node ids
and are unaffected. A map with arches would have perhaps 1.05 nodes per column,
not 2, so none of the latencies below would move much.

### 9.3 Step 2 — ground truth

Identical at all three cluster sizes (it does not use the decomposition):
**1 000 pairs compared against an exhaustive Dijkstra from 27 sources,
0 mismatches**, plus 201 pairs where both agreed the goal is unreachable. The
heuristic is admissible; every accuracy number below rests on a verified
baseline, as §3 step 2 demands.

### 9.4 The results table — every row, all three cluster sizes

Seed 20 260 913, 300 units, crater radius 3, ≥20 000 repaths, 10 000 estimate
queries, 2 000 accuracy pairs. **Cluster-32 latency rows are the median of three
whole-bench runs** (`results/bench-c32-run{1,2,3}.json`); cluster 16 and 64 rows
are from `results/bench-c{16,64}.json` with the second run quoted where it
differs materially. Accuracy rows are deterministic and come from a single run
each (`results/accuracy-c{16,32,64}.json`).

| Number | **16** | **32** | **64** | Gate part |
|---|---|---|---|---|
| Walkable cells / columns | 147 456 / 147 456 | 147 456 / 147 456 | 147 456 / 147 456 | scale |
| Directed steps, pristine → after the stream | 990 678 → 821 654 | 990 678 → **859 322** | 990 678 → 877 082 | scale |
| Largest component, pristine → after | 130 937 → 10 386 | 130 937 → **35 461** | 130 937 → 41 450 | scale |
| Components, pristine → after | 4 234 → 5 786 | 4 234 → **5 309** | 4 234 → 5 151 | scale |
| Abstract nodes, pristine → after | 6 987 → 8 686 | **3 306 → 4 072** | 1 451 → 1 716 | scale |
| Abstract directed edges, pristine → after | 51 520 → 40 292 | **38 494 → 32 412** | 23 708 → 16 972 | scale |
| Clusters | 576 | **144** | 36 | scale |
| Abstract graph build (once, at load) | 137 ms | **246 ms** | 389 ms | — |
| HPA\* excess over optimal p50/p90/p99/max | 1.2 / 3.1 / 12.5 / 104.7 % | **1.0 / 3.6 / 22.7 / 145.2 %** | 0.4 / 2.8 / 32.3 / 145.2 % | feeds the estimate error |
| …p90 by band ≤32 / 32–96 / 96–256 / >256 | 7.0 / 3.5 / 2.7 / 2.3 % | **9.2 / 5.0 / 3.1 / 2.6 %** | 3.8 / 4.1 / 2.8 / 2.2 % | ” |
| **Repath cold** p50/p90/**p99**/max | 0.167 / 0.688 / **1.076** / 1.544 ms | 0.492 / 1.122 / **1.836** / 2.443 ms | 1.373 / 2.947 / **3.651** / 5.318 ms | **G2-a: p99 ≤ 5** ✅✅✅ |
| Repath cold, n | 2 605 | 1 913 | 1 652 | — |
| Repath warm p50/p90/p99/max | 0.405 / 0.936 / 1.208 / 4.181 ms | 0.817 / 1.407 / 1.752 / 5.730 ms | 1.963 / 3.122 / 3.723 / 11.895 ms | context |
| All searches p50/p90/p99/max | 0.378 / 0.917 / 1.196 / 4.181 ms | 0.791 / 1.398 / 1.768 / 5.730 ms | 1.921 / 3.112 / 3.715 / 11.895 ms | context |
| "No path" (oracle short-circuit) p50/p99/max | 0.0001 / 0.0003 / 0.0005 ms | 0.0001 / 0.0003 / 0.0013 ms | 0.0001 / 0.0004 / 0.0012 ms | cheap, as §3 step 4 demands |
| No-path results / of repaths | 1 477 / 20 000 | 896 / 20 000 | 867 / 20 014 | ” |
| Walkers sealed in (buried, re-seeded) | 7 | 1 | 0 | ” |
| Repath cold **+ this tick's repair share** p50/p90/p99/max | 0.515 / 1.022 / 1.431 / 4.283 ms | 1.012 / 1.644 / **2.948** / 8.871 ms | 2.655 / 4.251 / **10.572** / 37.098 ms | the walker's real wait |
| **Cluster repair**, per dirtied cluster p50/p90/p99/max | 0.186 / 0.333 / 0.444 / 1.387 ms | 1.480 / 2.502 / **3.300** / 4.022 ms | 8.007 / 16.477 / **18.474** / 29.889 ms | churn cost |
| Dirtied clusters per crater (mean) | 1.74 | 1.35 | 1.16 | ” |
| Graph (CSR) rebuild, per tick p50/p90/p99/max | 0.674 / 0.725 / 0.808 / 2.466 ms | 0.499 / 0.539 / 0.593 / 1.460 ms | 0.316 / 0.348 / 0.389 / 0.873 ms | churn cost |
| Repaths/s at peak destruction (game-seconds) | 1 183 /s | **1 474 /s** | 1 138 /s | headroom |
| …the same run in wall-clock seconds | 2 450 /s | 1 262 /s | 520 /s | ” |
| **Estimate query** p50/p90/**p99**/max | 0.128 / 0.500 / **0.972** / 1.390 ms | 0.291 / 0.505 / **0.820** / 1.032 ms | 0.984 / 1.207 / **1.433** / 1.833 ms | **P3-a: ≤ 1** ⚠️✅❌ |
| …p99 **within the >256-cell band alone** | **1.076 ms ❌** | 0.887 ms ✅ | 1.499 ms ❌ | P3-a, the case that matters |
| …p99 by band ≤32 / 32–96 / 96–256 | 0.303 / 0.407 / 0.658 ms | 0.363 / 0.464 / 0.614 ms | 1.306 / 1.275 / 1.289 ms | ” |
| Estimate, cold abstract graph after a repair | 0.113 / 0.386 / 0.727 / 1.427 ms | 0.294 / 0.493 / 0.747 / 0.817 ms | 0.939 / 1.192 / 1.371 / 1.607 ms | P3-a, the Lull case |
| Abstract expansions per query p50/p90 | 351 / 2 052 | 169 / 999 | 68 / 431 | levels evidence (§9.7) |
| Endpoint-sweep expansions p50/p90 | 476 / 512 | 1 774 / 2 048 | 6 515 / 7 880 | ” |
| **Estimate vs walked**, signed p50/**p90**/p99/max | +1.5 / **+3.8** / +20.0 / +127.2 % | +0.7 / **+2.6** / +20.7 / +66.6 % | +0.2 / **+1.6** / +14.9 / +58.9 % | **P3-b: p90 ≤ ±15** ✅✅✅ |
| …p90 by band ≤32 / 32–96 / 96–256 / >256 | 9.5 / 3.7 / 2.7 / 2.2 % | 9.2 / 2.8 / 1.8 / 1.3 % | 4.1 / 2.2 / 1.6 / 1.0 % | where the error lives |
| Estimate vs optimal, signed p50/p90/p99/max | +3.0 / +6.5 / +25.8 / +127.2 % | +2.0 / +6.6 / +38.4 / +177.7 % | +0.9 / +4.6 / +47.1 / +177.7 % | diagnosis |
| Smoothing gain (walk vs unsmoothed) p50 / best | −1.5 % / −56.7 % | −0.7 % / −40.6 % | −0.2 % / −36.7 % | why the bias is one-sided |
| **Fogged-route** \|error\| (×1.5 rule) p50/p90/p99/max | 25.8 / 42.2 / 62.5 / 213.6 % | 23.7 / **48.6** / 66.1 / 130.3 % | 21.5 / 49.8 / 63.8 / 137.1 % | information for S3 |
| …fog inflation over the clear estimate p50/p99 | 23.8 / 50.0 % | 22.5 / 50.0 % | 20.7 / 50.0 % | ” |
| **Dynamic-terrain** \|error\| p50/p90/p99/max | 0.0 / 16.3 / 80.0 / 416.6 % | 0.0 / **12.5** / 62.5 / 400.0 % | 0.0 / 9.0 / 47.3 / 220.0 % | information for S3 |
| Memory: structures total | 4 418 KiB | **4 111 KiB** | 3 844 KiB | fits alongside sim + mesher |
| …of which abstract graph (CSR) | 827 KiB | 592 KiB | 362 KiB | ” |
| …of which world + node maps | 1 152 KiB | 1 152 KiB | 1 152 KiB | ” |
| …of which low-level scratch / abstract scratch | 2 304 / 136 KiB | 2 304 / 64 KiB | 2 304 / 27 KiB | per-query scratch |
| Memory: peak live heap (counting allocator) | 11 811 KiB | **11 224 KiB** | 10 327 KiB | ” |
| Memory: **peak working set, read externally** | 15 436 KiB | **14 528 KiB** | 13 548 KiB | the number a budget is written against |
| Allocations per query, estimate / path | 2 per 10 000 / 1 per 1 000 | **1 per 10 000** / 2 per 1 000 | 1 per 10 000 / 2 per 1 000 | §5 "zero in the steady state" ✅ |
| A\* vs Dijkstra exactness | 1 000 pairs, **0 mismatches** | 1 000 pairs, **0 mismatches** | 1 000 pairs, **0 mismatches** | step 2 ✅ |
| Path identity, same machine | yes — digest `04d3a2cf a0f051ce` | yes — digest `360bb858 b87df6e9` | yes — digest `f4f34e2d 55006508` | protects G4 |
| Path identity, cross-OS | **pending CI** | **pending CI** | **pending CI** | protects G4 |
| Destruction stream: ticks / craters | 3 462 | 2 339 | 2 028 | context |

A caution about the "after the stream" columns: **the three cluster sizes did not
receive the same amount of destruction.** The run stops at 20 000 repaths, not at
a fixed tick count, and a cluster size that repaths more often per crater reaches
that budget sooner — 3 462 craters at 16, 2 339 at 32, 2 028 at 64. So the
post-destruction graph sizes and component counts are *not* comparable across the
three columns; only the pristine ones are. Every latency and accuracy row is
unaffected, because each is a distribution over that column's own run.

### 9.5 The verdicts, per cluster size and overall

**G2-a — repath p99 ≤ 5 ms (cold).**

- **16: pass, 1.08 ms** (second run 1.09 ms), 4.6× under budget.
- **32: pass, ~1.84 ms** (three runs: 1.799 / 1.840 / 1.836), 2.7× under budget.
- **64: pass, 3.65 ms** (second run 3.68 ms), 1.4× under budget — but the tail is
  thin: warm max reached 11.9 ms and 13.1 ms in the two runs, and
  *cold + the tick's repair share* has p99 10.6 ms and max 37.1 ms, i.e. the
  walker's actual wait blows the budget at 64 even though the search alone does
  not.
- **Overall: pass**, comfortably, at every size that also passes P3-a.

**P3-a — estimate query ≤ 1 ms.**

- **16: aggregate pass, 0.972 ms — but a fail on long routes.** Within the
  >256-cell band the p99 is **1.076 ms** and the max 1.390 ms. The aggregate
  passes only because three of the four stratification bands are short routes
  that cost 0.07–0.19 ms each. An editor estimating a cross-map march is in the
  failing population, so 16 should be read as *not* meeting P3-a.
- **32: pass, 0.820 ms** (three runs 0.823 / 0.820 / 0.815), 18% of headroom on
  the aggregate and 11% within the worst band (0.887 ms p99). The band max
  1.03 ms does cross 1 ms occasionally; the gate is stated at p99 and p99 passes
  everywhere.
- **64: fail, 1.43 ms** — 43% over budget, and *every* band fails, because at 64
  the two bounded endpoint-insertion sweeps alone cost 6 515 expansions at the
  median.
- **Overall: pass at 32 only** (16 passes the letter and fails the spirit).

**P3-b — |error| p90 ≤ 15% vs walked, on static terrain.**

- **16: pass, 3.8%.** **32: pass, 2.6%.** **64: pass, 1.6%.**
- The gate is met by 4–9× at every cluster size, and the ordering is monotone:
  coarser clusters give a *more* accurate estimate, because a coarser abstract
  graph has fewer, longer edges whose cost was computed by a real intra-cluster
  search. Accuracy is therefore **not** the constraint on cluster size; latency
  is. That is the single most useful thing this sweep says.
- **Overall: pass**, with the two caveats in §9.6.

**The headline decision the gate exists to make:** the editor promises a **travel
time**. At the recommended cluster 32 the estimate is within 2.6% at p90 and
within 1.3% at p90 on routes over 256 cells — the routes an ETA is actually read
for — against a budget of 15%. §6's fallback ("show ranges; favour arrival
triggers") is not taken.

### 9.6 The systematic bias: consistently high, and no calibration constant

The estimate is **never optimistic on static terrain**, at any cluster size. At
every percentile reported, the *signed* error equals the *absolute* error
exactly — p50, p90, p99 and max — at 16, 32 and 64. That is not a lucky sample;
it is structural:

- `estimate()` returns the summed cost of the abstract path, and a test
  (`estimate::the_estimate_prices_the_unsmoothed_refinement_exactly`) pins that
  to the cost of the **unsmoothed** refinement;
- the smoother only ever removes cost — measured `smoothing_gain_pct` p50 −0.7%
  at cluster 32, best case −40.6%;
- so `estimate ≥ walked` always, and the error is a one-sided over-estimate.

**Would a calibration constant fix it?** It would move the median and buy very
little, at a cost that is not worth paying:

- The constant is not a property of the world, it is a property of the
  decomposition: the median bias is +1.5% at cluster 16, +0.7% at 32, +0.2% at
  64. It would have to be re-fitted whenever the cluster size or the smoother's
  `LOOKAHEAD` changed.
- Subtracting it would move p90 from 2.6% to roughly 1.9% at cluster 32 — against
  a 15% budget that is already met by 5.8×.
- It would destroy the property the editor actually wants. "Orders you cannot
  take back" makes an ETA that is *never late* categorically better than an ETA
  that is 0.7% closer on average and sometimes optimistic. A unit arriving
  slightly early is a pleasant surprise; a unit arriving after the window the
  player planned around is a broken promise.

**Recommendation: no calibration constant.** The residual error has a structural
cause worth fixing instead, and it is cheap: **the abstract graph carries no
corner entrance.** A diagonal step across a cluster corner is legal for a walker
but has no transition node, so the estimate prices it as two cardinal crossings
(≥ 20 cost) where the walker cuts the corner for 14. That is most of why the
≤32-cell band has p90 9.2% and max 66.6% against 1.3% p90 over 256 cells: on a
short route, one corner is a large fraction of the whole. Emitting one transition
at each of a cluster's four corners where the diagonal is legal would fix it
without touching the dirty set, since a corner's legality depends only on the
four columns around it. Recorded here, not implemented.

**What the editor must do about the residual.** Round a short route's ETA
generously, or show it only in whole seconds. A 66% error on a route that takes
two seconds is one second; a 1.3% error on a four-minute march is three seconds.
Both are fine to render as a number; the failure mode is rendering
"0:04" for something that takes six.

### 9.7 Fog (step 7) and dynamic terrain (step 8), recorded separately

Neither is in the gate — §3 says the gate is static terrain — but both are the
conditions a player is actually in, so they are recorded on their own terms.

**Fog, ×1.5 on unknown cells.** Mask: 24-cell blocks in a checkerboard covering
half the map, so every route of any length crosses several known/unknown
boundaries rather than short routes missing the mask entirely.

| | 16 | 32 | 64 |
|---|---|---|---|
| \|error\| vs walked p50 / p90 / p99 / max | 25.8 / 42.2 / 62.5 / 213.6 % | 23.7 / **48.6** / 66.1 / 130.3 % | 21.5 / 49.8 / 63.8 / 137.1 % |
| Inflation over the clear estimate p50 / p90 / p99 | 23.8 / 36.9 / 50.0 % | 22.5 / 46.7 / 50.0 % | 20.7 / 49.5 / 50.0 % |

A fogged estimate is **three times outside what the gate asks of a clear one**,
and the distribution barely moves with cluster size — it is dominated by the
×1.5 rule itself, not by the decomposition. The inflation p99 of exactly 50.0% is
the signature of the rule saturating: on those routes every abstract edge was
priced as fogged. The honest reading is that a fogged leg's number is not an ETA,
it is an upper bound with a made-up constant in it, and §13's dashed rendering
should be understood as carrying that meaning.

**Dynamic terrain**, measured the way a player experiences it: the estimate a
walker was given when its route was issued, against the tick it actually arrived
on after however many repaths and detours the destruction stream forced.

| | 16 | 32 | 64 |
|---|---|---|---|
| \|error\| p50 / p90 / p99 / max | 0.0 / 16.3 / 80.0 / 416.6 % | 0.0 / **12.5** / 62.5 / 400.0 % | 0.0 / 9.0 / 47.3 / 220.0 % |
| Samples (arrivals) | 3 600 | 2 012 | 1 551 |

The median is **zero** — most walkers arrive exactly when they were told, because
most craters miss most routes. The damage is concentrated: p90 12.5% at cluster
32 is still inside what the gate asks of static terrain, p99 is four times
outside it, and the maxima are walkers that were sealed in and re-seeded. Note
that 16 degrades more than 64 for the same reason it repaths more often: smaller
clusters mean more of the route is invalidated per crater. So under the
destruction a *coarser* decomposition gives the steadier promise, which is the
opposite of the latency ordering.

### 9.8 The spread: three cluster-32 runs of the same binary and seed

| run | repath cold p99 | estimate p99 | route digest |
|---|---|---|---|
| 1 | 1.799 ms | 0.823 ms | `360bb858b87df6e9` |
| 2 | 1.840 ms | 0.820 ms | `360bb858b87df6e9` |
| 3 | 1.836 ms | 0.815 ms | `360bb858b87df6e9` |
| (4th, under the memory sampler) | 1.801 ms | 0.821 ms | `360bb858b87df6e9` |

**Spread 2.3% on the repath p99 and 1.0% on the estimate p99**; the tail moves
much more than that (cold max 2.150 / 2.443 / 4.148 ms across the three runs,
warm max 2.152 / 5.730 / 6.108 ms). So: quote the p99s to three digits, and
never quote a max as if it were a property of the code. All four runs walked
byte-identical routes.

The cold distribution is also **not stationary within a run**. By run quartile
(run 1, cluster 32):

| quartile | 1 | 2 | 3 | 4 |
|---|---|---|---|---|
| cold p50 | 0.768 ms | 0.526 ms | 0.396 ms | 0.299 ms |
| cold p99 | 1.657 ms | 1.629 ms | 1.920 ms | 1.160 ms |

The median falls by 2.6× as destruction shreds the map into shorter routes and
smaller components, while the tail wanders. Every slice passes G2-a with room.

### 9.9 What could not be measured, and what was measured differently

Stated rather than buried, because each of these is a place where the number
above does not mean quite what §3 asked for.

1. **Peak RSS was measured from outside the process, not by the harness.** §3's
   table asks for "peak RSS"; the harness reports peak *live heap* from a
   counting wrapper around the system allocator, because reading true RSS needs
   `GetProcessMemoryInfo` on Windows and therefore a `windows-sys` dependency the
   spike is not allowed (§3 step 0). The external number in the table comes from
   PowerShell polling `Process.PeakWorkingSet64` every 40 ms while the process
   runs — note that after the process exits the property reads **0**, so it must
   be sampled live, which is why the recipe in §9.1 is a loop and not a one-liner.
   At cluster 32 the two numbers are 11 224 KiB (live heap) and 14 528 KiB
   (working set); the ~3.3 MiB difference is code, stacks and CRT. The
   `PLACEHOLDER` in `bench.rs` — whether the sim's own bench harness may take
   that dependency — is still the owner's, at the S3 gate.
2. **Cross-OS path identity is not measured.** `spikes/g2-pathing/ci/spike-g2.yml`
   is written but is **not installed** in `.github/workflows/`, so no Linux run
   exists. Same-machine identity is established four ways over (§9.8, and the two
   `--hash-paths` runs below); the Linux half reads **pending CI** and is not
   guessed at.
3. **Step 9's byte-for-byte check passed.** Two `--hash-paths` runs at cluster 32
   produced files identical under `cmp` — 55 030 bytes, 3 200 hashed lines,
   MD5 `6c369cff69cbbef287fd788c86f87b83`, internal digest `ef29afabb1d26148`.
   Cluster 16 digest `d98ba7776dc48e9c`, cluster 64 `8daf1375f13e8619`. The file
   covers both the pristine map (2 000 stratified path hashes) and the repair
   path (for each of 300 craters, the decomposition's `fingerprint()` plus three
   repaths on the graph that came out, 31 of which are `nopath`) — i.e. the
   entrance rescan, the intra rebuild, the CSR and the union-find relabel are all
   inside the comparison, not just the static queries.
4. **The fog number measures a per-edge-endpoint rule, not the per-cell rule §3
   states.** The estimator applies ×1.5 to an abstract edge when *either*
   endpoint cell is unknown, because walking an edge's cells to count how many
   are unknown is precisely the refinement §11 forbids the estimator to do. The
   literal per-cell rule is unmeasured; 48.6% p90 is the error of the
   approximation S3 would actually be implementing.
5. **"Cold" does not mean what the plan expected.** §3 step 4 assumes the first
   repath after a repair is slower. There is no per-query cache anywhere in the
   design — the scratch arrays are generation-stamped and re-stamped on every
   query — so re-issuing a query is the same measurement twice, not a warm one.
   The distinction that does exist is the abstract graph: *cold* = the first
   repath against a CSR rebuilt on this very tick, *warm* = every later repath
   against that unchanged graph. Measured, cold is **3–5% worse at the p99 and
   40% better at the median** than warm (0.492 vs 0.817 ms p50 at cluster 32),
   because cold samples cluster at the moments when routes are shortest. What the
   walker actually pays for the destruction is the **repair**, not a cold search:
   cold-plus-repair-share is 2.95 ms p99 against 1.84 ms for the search alone.
6. **A second abstract level was not implemented.** §2 asked for evidence rather
   than intuition about whether one earns its keep; the evidence taken is the
   split of a query's expansions (§9.7 rows, and §9.10 item 1), which bounds what
   a second level *could* save without building one.
7. **The walkable-cell count is 100% by construction** and the headroom rule is
   never exercised — see §9.2. A real generator with buildings, cliffs and
   ceilings would exercise it, and would have fewer nodes, not more.
8. **The three cluster sizes saw different amounts of destruction** (3 462 /
   2 339 / 2 028 craters), because the run terminates on a repath budget. The
   post-destruction scale rows are therefore not comparable across columns; see
   the note under §9.4.
9. **One bench run per cluster size at 16 and 64** was taken under the PowerShell
   memory sampler. Its 40 ms polling loop is visible in neither distribution
   (cold p99 1.076 vs 1.091 ms at 16, 3.651 vs 3.676 ms at 64), but those runs
   are used only for the memory row.

### 9.10 Decision candidates (plan §8), for the owner to approve

§8 asks this spike for one decision — *does the editor promise a travel time, or
a range?* — and the five parts of the pathing design that answer rests on. Each
item below gives the candidates the measurement actually leaves open, a
recommendation, and what the alternatives cost. **Nothing here has been written
to `docs/design/decisions-log.md` §2.7 — that file is a contract file
(AGENTS.md §5) and the entry is the owner's to make.** Item (5) is part of the
determinism contract and needs owner approval alongside G4's.

**(0) The headline: a time, or a range?**

- **(Recommended) The editor promises a travel time**, rendered as a number, on
  static terrain, with two qualifications it must honour: short routes are
  rounded generously or shown in whole seconds (≤32-cell band p90 9.2%, max
  66.6%), and fogged legs are drawn as the upper bound they are, not as an ETA
  (|error| p90 48.6%). Measured 2.6% p90 against a 15% budget; 1.3% p90 on routes
  over 256 cells.
- *Downside:* the promise is made on static terrain, and the Push is not static.
  Under the destruction stream the same estimate is 12.5% p90 and 62.5% p99 — so
  the number is honest when the playbook is sealed and can be badly wrong by the
  time the walker arrives. The player sees the discrepancy, not the caveat.
- *Alternative — show a band anyway (§6's fallback).* Downside: it discards a
  measurement that came in 5.8× under budget, and a band is strictly less useful
  for authoring against the clock; the playbook vocabulary would drift towards
  arrival triggers for no measured reason.
- *Alternative — a number on clear routes, a band on fogged or contested ones.*
  Downside: two renderings to design, document and teach, and the rule for which
  one applies ("is this route contested?") is not something the estimator can
  answer without doing work §11 forbids. Defensible if the owner wants the fog
  caveat to be visible rather than documented.

**(1) Cluster size, and how many abstract levels.**

- **(Recommended) Cluster 32, one abstract level.** It is the only size that
  passes all three assertions including within the long-route band; its cluster
  footprint is a chunk's, so "an explosion dirtied chunk (cx, cz)" is "repair
  cluster (cx, cz)" with no translation; and it costs 1.48 ms p50 to repair one
  dirtied cluster, which fits a 50 ms tick alongside everything else.
- *Downside of 32:* estimate p99 0.82 ms leaves 18% of the P3-a budget, and a
  busier map or a slower target machine eats that. The band max touches 1.03 ms
  already.
- *Alternative — 16.* Fastest repath (1.08 ms p99) and cheapest repair (0.19 ms
  p50, 7.5× cheaper than 32). *Downside:* estimate p99 0.972 ms aggregate and
  **1.076 ms within the >256-cell band** — it fails P3-a on exactly the routes an
  ETA is read for. Also twice the abstract graph to keep repaired (6 987 nodes,
  51 520 edges), the highest per-tick CSR rebuild cost (0.67 ms), the worst
  dynamic-terrain error (16.3% p90), and a cluster that is no longer a chunk.
- *Alternative — 64.* Best accuracy (1.6% p90) and the smallest graph.
  *Downside:* **fails P3-a by 43%** (1.43 ms p99, every band failing), 18.5 ms
  p99 to repair a single dirtied cluster — nearly four times the whole repath
  budget — and a walker's cold-plus-repair wait of 10.6 ms p99, 37 ms max. Not
  viable.
- **One abstract level, and the number that says so.** A second level can only
  make the *abstract* search cheaper. It cannot touch the two endpoint-insertion
  sweeps, which are cluster-local by construction and capped at 2 × 32² = 2 048
  expansions — a cap they hit by p90. At cluster 32 the abstract search is 169 of
  1 943 median expansions, so the ceiling on what a second level could save is
  **9% of a median query** and about a third of a p90 one, against an estimate
  p50 already 3.4× under budget. It does not pay. *Downside of not having one:*
  if a later map is much larger than 384², the abstract half grows and this
  arithmetic changes — the ratio, not the conclusion, is what should be carried
  forward. Note the trade runs the other way at 64 (68 abstract expansions
  against 6 515 sweep expansions), which is exactly why coarsening fails.

**(2) The integer cost model.**

- **(Recommended)** `STEP_CARDINAL = 10`, `STEP_DIAGONAL = 14` (the standard
  octile approximation, so no square root and no float), `CLIMB_SURCHARGE = 4`
  for a one-voxel step up or down, `MOVE_COST_PER_TICK = 3`, and cost→ticks by
  **`ceil`**: `ticks = (cost + 2) / 3`. All of these ship as **rules-table data**,
  not constants in code (AGENTS.md §12), and are stamped into the rules hash.
- *Why `ceil` rather than round-to-nearest:* it is the rounding that keeps the
  estimator's one-sided property (§9.6) — a never-optimistic ETA — and it is
  what the walk integrator is tested against
  (`hpa::the_integrator_agrees_with_the_rounding_rule`). *Downside:* it adds up
  to one tick (50 ms) of pessimism per query, which is invisible next to the
  measured 0.7% median bias but is a systematic floor under it.
- *Alternative — a finer cost scale (100/141 instead of 10/14).* Downside: ten
  times the accumulated magnitude for no measured accuracy gain; the cost bound
  is 2 654 208 today against `i32::MAX`, and 810× headroom is comfortable rather
  than extravagant. The octile error is not what limits accuracy here — the
  missing corner entrance is.
- *Alternative — a larger `CLIMB_SURCHARGE`, or an asymmetric one (up costs more
  than down).* Downside: it is a gameplay tuning question, not a pathing one, and
  an asymmetric cost makes the graph directed in a way the current CSR and the
  connectivity oracle would both have to learn. Raise it as tuning if climbing
  should feel expensive; the spike has no evidence either way.

**(3) Incremental repair: strategy and granularity.**

- **(Recommended) Cluster = chunk footprint (32²), repaired by rescanning the
  dirty clusters' entrances plus the precise affected-neighbour set, then one CSR
  rebuild per tick.** "Affected neighbour" is computed, not assumed: a border is
  rebuilt and the neighbour is enqueued only if the border's transition set
  actually changed. Three tests pin it — repair produces a graph identical to a
  from-scratch build, identical to the conservative all-neighbours set, and
  identical at every cluster size.
- Measured at 32: 1.48 ms p50 / 3.30 ms p99 per dirtied cluster, 1.35 dirtied
  clusters per crater, plus 0.50 ms p50 for the one CSR rebuild per tick.
- *Downside:* repair, not the search, is what the walker waits for — cold + the
  tick's repair share is 2.95 ms p99 against 1.84 ms for the search alone, and
  its max reached 8.9 ms. At 20 explosions per second this is the cost that will
  compete with the mesher and the rest of the tick, and G3′ has not yet fixed the
  tick budget these have to fit inside.
- *Alternative — repair lazily, on the first query that touches a dirty cluster.*
  Downside: it moves an amortised cost into a latency spike exactly when many
  units repath at once, and it makes the graph's contents depend on query order,
  which is a determinism hazard for something that is hashed sim state.
- *Alternative — a smaller repair granularity than a whole cluster (rebuild only
  the affected entrances' intra edges).* Downside: the intra rebuild is the
  expensive half and it is what produces the edges; a partial rebuild needs a
  dependency structure that does not exist yet, and the measured 1.48 ms does not
  yet justify inventing one. Worth revisiting only if S3's tick budget says so.
- *Alternative — coarsen the repair by deferring: let units run on a stale path
  for a bounded number of ticks.* Downside: it is §6's G2-a fallback and G2-a
  passed by 2.7×; it buys nothing today and costs a visible "walking into a
  crater" artefact.

**(4) The estimator's API and error contract.**

- **(Recommended)** The estimator returns a **tick count**, not a range and not a
  distribution: `Estimate { cost, ticks, legs }` from
  `estimate(world, clusters, scratch, start, goal, fog) -> Option<Estimate>`,
  where `None` means "no route" and is answered by the connectivity oracle before
  any search (measured 0.0001 ms — a normal, cheap answer, as §3 step 4 demands,
  because a moat seals you in as well). `legs` is how many abstract edges the
  route crossed, which is what the editor would use to decide how many polyline
  legs to draw.
- **Error contract:** the gate's ±15% at p90 against walked time, **on static
  terrain**. Measured 2.6% — the 12.4 points of margin are headroom S3 gets to
  spend on the difference between this toy and the real sim, not accuracy to
  bank. The estimate is **never optimistic** (§9.6); that property should be
  written into the contract, because the editor's rounding depends on it.
- **Fog is priced ×1.5 per abstract edge when either endpoint is unknown**, and
  what the editor may render from a fogged leg is *not* an ETA. *Downside:* this
  is an approximation of §11's per-cell rule, chosen because the per-cell rule
  needs the refinement the estimator is defined not to do; its error is 48.6%
  p90, and the owner may prefer the editor to show fogged legs as a band or as
  "unknown" rather than as a number with a dashed line.
- *Alternative — return a (low, high) pair always.* Downside: it throws away the
  measurement, and the "high" would be the ×1.5 fog bound which is arbitrary. But
  it is the honest rendering for the dynamic case, and the owner may want the
  contract to carry both a point and a bound so the editor can choose.
- *Alternative — cache estimates per (start-cluster, goal-cluster) and invalidate
  on repair.* Downside: unnecessary today (P3-a passes with 18% of headroom at
  the recommended size) and it is state that has to be invalidated correctly or
  it becomes a desync. Keep it in the pocket as §6's named P3-a fallback.

**(5) The total-order tiebreak — part of the determinism contract.**

- **(Recommended)** Search nodes are ordered by **`(f, h, node_id)`**, ascending
  in all three, in a hand-written binary min-heap. `f = g + h`; ascending `h`
  among equal `f` prefers the node further along; `node_id` last makes the order
  total, so no two distinct nodes ever compare equal and the pop sequence is a
  function of the graph and nothing else. Never compare on cost alone, and every
  sort key anywhere (entrances, neighbours, dirty sets) ends in a unique id.
- Evidence: `astar::the_tiebreak_is_total_under_neighbour_permutation` passes;
  four whole-bench runs at cluster 32 walked byte-identical routes; two
  `--hash-paths` runs are identical under `cmp`.
- *Downside / what is still open:* this is **same-machine only**. The cross-OS
  half is the first CI run of `ci/spike-g2.yml`, which is not installed. The
  argument that it will hold is structural — dense arrays rather than hash maps,
  `u32` node ids rather than `usize`, integers throughout, no recursion, a total
  order with no equal keys — but it is an argument, not a measurement.
- *Alternative — order by `(f, node_id)` only.* Downside: still total, still
  deterministic, but it discards the goal-ward preference and expands more nodes
  for the same answer. There is no reason to prefer it.
- *Alternative — `std::collections::BinaryHeap`.* Downside: this is the trap §5
  names. It gives no order among equal keys, and the order it does produce
  depends on insertion history and on the std implementation — two platforms can
  return different, equally optimal paths. Since paths are hashed sim state, that
  breaks G4. A total-order key makes the container's behaviour irrelevant, which
  is the point; the spike wrote its own heap anyway because it takes no
  dependencies.

### 9.11 Lessons for the skeleton

What surprised the measurement, and what the real crate must do differently.

- **Accuracy was never the risk; latency was.** The design dependency this gate
  exists to resolve — "can the editor show a number?" — came in at 2.6% against a
  15% budget, i.e. it was never close. Meanwhile the assertion that actually
  discriminates between designs is P3-a, the 1 ms estimate, which cluster 64
  fails outright and cluster 16 fails on long routes. The sweep is monotone in
  opposite directions: coarser is more accurate and slower to query, finer is
  faster to query and cheaper to repair but less accurate and more graph to
  maintain. S3 should budget its attention accordingly.
- **The estimate's cost is dominated by endpoint insertion, not by the abstract
  search.** At cluster 32, inserting the start and goal as temporary nodes costs
  1 774 expansions at the median against 169 for the abstract search itself —
  91% of the work. Every intuition about "make the abstract graph smaller to make
  queries faster" is backwards here: shrinking the graph grows the sweeps
  quadratically in cluster area, which is the whole story of why 64 fails. If S3
  wants a faster estimate, the thing to attack is the endpoint sweep (cache the
  start's transitions while the commander stands still; the goal is usually a
  beacon, which is static), not the abstract search.
- **The repair is what the walker waits for, not the search.** Cold repath p99 is
  1.84 ms; cold-plus-the-tick's-repair-share p99 is 2.95 ms and max 8.9 ms. The
  real crate's tick budget must account for repair, and the "repath p99" number
  alone will under-report the cost by ~60%. It also means the repair, not the
  search, is what to parallelise — except that it writes sim state, so it cannot
  be (AGENTS.md §4.6). Coarsening the repair is the lever, and 32 is already the
  right coarseness.
- **"Cold" was the wrong mental model.** The design holds no per-query cache, so
  there is nothing to be cold. Anything the real crate adds that *is* a cache —
  an estimate cache, a route cache, a per-unit path memo — reintroduces the
  cold/warm distinction and, worse, becomes hashed state that has to be
  invalidated identically on every platform. The spike's evidence is that none is
  needed at cluster 32; adding one should have to justify itself against these
  numbers.
- **Walkability is not where the graph's structure lives; the step rule is.** The
  generator's 100%-walkable columns were a shock only until the directed-step
  count was reported. 20 explosions per second removed 71 columns and 131 356
  directed steps, and shattered the largest component 130 937 → 35 461. The real
  sim should hash and report **steps and components**, not cell counts, if it
  wants a number that reflects what pathing sees.
- **"No path" has to be a first-class cheap answer, and it fires often.** 896 of
  20 000 repaths at cluster 32 — 4.5% — returned no route, and at cluster 16 it
  was 7.4%. A connectivity oracle (union-find over the abstract graph, relabelled
  during repair) answers those in 0.0001 ms, against ~1.8 ms for a search that
  would exhaustively fail. Without it the p99 of the *whole* repath population
  would be an exhaustive component sweep. The real crate needs this from day one,
  not as an optimisation later.
- **Sealed-in walkers are real and must be a designed state, not an error.** One
  walker at cluster 32 and seven at 16 ended up buried by their own destruction.
  §9 of the spec says a moat seals you in as well; the sim needs a defined
  behaviour (park, re-issue, report to the seat) rather than a repath loop.
- **A single timing run is not a number, and a max is not a property of the
  code.** Three identical runs spread 2.3% on the p99 and 93% on the max
  (2.150 → 4.148 ms). Any CI assertion must be written against a distribution and
  a margin that says it is a regression alarm, not a certification — which is
  what `ci/spike-g2.yml` does, and its threshold is still a `PLACEHOLDER` for the
  owner at the S3 gate.
- **The lint wall worked exactly as the plan hoped, and is worth copying.** The
  library is sim code and stays clean under the product's determinism set — no
  floats, no `as` casts, no `HashMap`/`HashSet`, no wall clock — with `Instant`
  confined to `src/bin/*.rs` and that confinement asserted by a test rather than
  by convention. The spike-local `clippy.toml` exists only because clippy walks
  ancestor directories and would otherwise apply the product's root file to a
  detached workspace. The real crate gets this for free from the workspace lint
  table; what it should copy is the *test* that the library reads no clock.
- **The one structural fix the numbers ask for is a corner entrance.** Four
  transitions per cluster, at the corners where the diagonal is legal, would
  remove most of the short-route error (≤32-cell band p90 9.2%, max 66.6%)
  without touching the dirty set. It is recorded here and deliberately not built,
  because it changes the abstract graph and this spike's job was to fix the
  design, not to optimise the toy.

### 9.12 The tuning values these numbers depend on

Every one is a spike-local constant that S3 must re-fix as a rules-table value
(AGENTS.md §12: "tuning values are data", stamped into the rules hash):
`STEP_CARDINAL = 10`, `STEP_DIAGONAL = 14`, `CLIMB_SURCHARGE = 4`,
`MOVE_COST_PER_TICK = 3` (so cost→ticks is `ceil(cost / 3)`), `HEADROOM = 2`,
`LONG_RUN = 6` (border runs longer than this get two transition nodes),
`LOOKAHEAD = 16` (the smoother's shortcut window), cluster size 32, crater radius
3, and — in the harness only — `FOG_BLOCK = 24`.

### 9.13 What still has to happen

- **The §8 decision needs its `docs/design/decisions-log.md` §2.7 entry.** That
  file is a contract file (AGENTS.md §5): the entry is an owner decision, raised
  in the PR, not written by an agent. §9.10 lays out the five items as candidates
  with recommendations so they can be taken one at a time; item (5) is part of
  the determinism contract and needs owner approval alongside G4's.
- **The cross-OS half of step 9** is the first CI run of `ci/spike-g2.yml`, which
  is not yet installed in `.github/workflows/`. Until it is green, "path
  identity" reads *same machine only*, and the Linux column is **pending CI** —
  not estimated.
- **The peak-RSS `PLACEHOLDER` in `bench.rs`** is the owner's at the S3 gate:
  whether the sim's own bench harness may take a `windows-sys` dependency to read
  its own working set, or whether peak live heap plus the structure sizes is the
  number the budget is written against. This spike measured it from outside the
  process instead.
- **The CI threshold `PLACEHOLDER` in `ci/spike-g2.yml`** (`5 ms` repath, `2.5 ms`
  estimate as regression alarms, not gates) is replaced at S3 by the budget the
  sim's own perf gate sets, once G3′ has fixed the tick budget these have to fit
  inside.
- **S3 still certifies this on real code.** Section 16 puts the prototype here
  and the certification in the sim; section 17 makes it S3's depth task. What
  this spike fixes is the design, not the number.
