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

*(Empty until the spike runs.)*

- Machine: `PLACEHOLDER` (CPU, cores, RAM, OS build) · Rust: `PLACEHOLDER`
- Map seed and walkable-cell count: `PLACEHOLDER`
- G2-a repath p99 (cold / warm), Windows / Linux: `PLACEHOLDER`
- P3-a estimate p99: `PLACEHOLDER`
- P3-b estimate vs walked, signed error p90: `PLACEHOLDER`
- Cluster-size sweep outcome: `PLACEHOLDER`
- Cross-OS path identity: `PLACEHOLDER`
- Verdict: `PLACEHOLDER` (go / which part of the fallback)
