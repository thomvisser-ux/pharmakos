<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# G3′ — Segment budget (spike, wk 1–3: the synthetic tick check)

## 1. Goal and go/no-go criterion

**Goal.** Find out, cheaply and early, roughly how much a 20 Hz tick can afford — so that the map generator's **per-map power budget** (which is what sets the unit ceiling: section 8 says the ceiling is *"a property of the world, not a rule: a seat can field only what its core surplus and vent Generators can power"*) is founded on a measurement rather than a hope, and so that the 3/5/8-minute segment ladder is known to be affordable.

**Go/no-go criterion, copied verbatim from the section 16 gate table:**

> **G3′ Segment budget** — Synthetic tick check in the spike; the real measurement is S2's exit; scrub at S7 — Go if: *"4 seats (headroom over v1's 3), 300 units, 40 beacons: tick p99 ≤50% of the tick; ≥4× speed on the headless sim runner. The measurement sets the per-map power budget for the generator. Scrub ≤1 s is judged at S7"* — Fallback: *"Lower map power budget; shorter segments"*

Converted into numbers:

| # | Assertion | Pass condition |
|---|---|---|
| G3′-a | Tick cost | tick p99 ≤ **50% of the tick**. The tick is 20 Hz = 50 ms, so **p99 ≤ 25 ms**. |
| G3′-b | Headless speed | **≥4×** real time on the headless sim runner. 4× of 20 Hz is **≥80 ticks/s**, i.e. a mean tick ≤ **12.5 ms**. |
| G3′-c | Load | at **4 seats** (headroom over v1's 3), **300 units**, **40 beacons**. |
| — | Scrub | **Not here.** "Scrub ≤1 s is judged at S7." |

Two things to hold onto, because they change how the result is read:

- **Note that G3′-b is the tighter constraint.** A mean of 12.5 ms is harder than a p99 of 25 ms for any workload with a reasonable spread. The spike reports both and says which binds.
- **This is the synthetic check, not the measurement.** Section 16 and section 17 both put the real one at S2's exit (*"exit: G3′ real measurement (300 units / 40 beacons), which sets the per-map power budget"*). The spike's job is to build the cost model — nanoseconds per unit per tick, per beacon per tick, per voxel edit — so that the power budget can be *provisionally* set now and re-set from real code later, and so that a catastrophic answer (an order of magnitude out) is found in week 3 rather than week 29.

**An open question for the owner, not to be guessed at.** The gate row reads "4 seats (headroom over v1's 3), 300 units, 40 beacons" without saying whether the counts are world totals or per-seat. The spike measures **world totals** (300 units and 40 beacons across 4 seats, i.e. 75 and 10 each) as the primary figure, and records the per-seat reading (1,200 units, 160 beacons) as a secondary stress number so the answer is available either way. The owner settles which the gate means; the spec does not say.

## 2. The toy build

`spikes/g3-tick-budget/` — a standalone Rust crate, no Godot, no rendering. It deliberately shares its shape with G4's toy sim: same SoA tables, same fixed-point types, same hashing. If G4 finishes first, this spike starts from a copy of it (a copy *within* `spikes/`, which the throwaway rule permits — the rule forbids copying spike code into `crates/`, not between spikes).

```
spikes/g3-tick-budget/
  Cargo.toml            # [workspace] (empty); release profile with overflow-checks = true
  src/
    tables.rs           # SoA: units, beacons, structures, projectiles, kill-credit counters
    tick.rs             # the synthetic tick, split into named phases
    broadphase.rs       # uniform grid over the map for range queries (r^2, Q32.32)
    power.rs            # kW supply/draw/brownout pass over 4 seats' grids
    quartermaster.rs    # integer $ arithmetic, one treasury per seat
    decision.rs         # decision tick: one per 250 ms of game time = every 5 ticks at 20 Hz
    voxels.rs           # 32^3 copy-on-write chunk store + destruction edits (sim side only)
    hash.rs             # per-tick xxh3 over the canonical encoding; full and incremental
  src/bin/
    tickbench.rs        # the measurement harness; wall clock lives here only
    costmodel.rs        # sweeps counts, fits per-entity costs, prints the power budget table
```

**What the synthetic tick does**, phase by phase, so the cost breakdown is meaningful rather than one opaque number:

| Phase | Work | Why it is in the budget |
|---|---|---|
| `broadphase` | rebuild/refresh the uniform grid; range queries in squared distance (Q32.32, no square roots) | section 15's maths row; the dominant cost in most RTS ticks |
| `programs` | each unit and building runs its built-in program (section 6) | *"every beacon mandate runs its built-in program and every unit and building runs its built-in program"* |
| `movement` | integrate Q16.16 positions, u16 headings through the 4096-entry angle table | |
| `combat` | damage, cooldowns, projectiles, friendly fire checks, integer HP | |
| `kill_credit` | per-seat integer counters (≤3 per asset) and largest-remainder apportionment | section 15 names these explicitly as hashed state |
| `power` | per-seat grid: supply, draw, brownout order with the beacon as the unit of power | S1's subject, but it ticks in every segment |
| `quartermaster` | integer $ settlement arithmetic | |
| `decision` | the playbook/mandate decision tick, **one per 250 ms of game time** — every 5th tick — for 4 seats | section 16's P1 row ties the per-rule cost to this cadence |
| `voxels` | apply the destruction edits of this tick to the copy-on-write chunk store | the sim half of G1's load |
| `hash` | per-tick xxh3 over the canonical state encoding | **this is part of the budget and is easy to forget** |

**The `hash` phase deserves its own warning.** Hashing the whole state every tick is O(state) and can plausibly be the single largest line item at 300 units and 40 beacons. The spike measures it in both forms — a full re-hash, and an incremental/merkle-ish scheme where each table keeps a dirty flag and only changed tables are re-hashed with the rest folded in from cached sub-hashes — and reports the cost of each. Whether the product's hash is incremental is one of this spike's decisions, and it is a **contract** decision because the hash is the determinism artefact G4 is establishing.

**Not in the toy**: the mesher and any rendering (those are G1's), the real pathfinder (that is G2's — the spike substitutes a fixed per-unit path-follow cost plus a configurable repath rate, using G2's measured repath cost once it exists), the verifier, and the gateway. The point is the sim tick, and padding it with things the sim tick does not do would produce a number nobody can act on.

## 3. Measurement procedure

**Step 0 — Prerequisites.** Rust toolchain, MSVC ABI on Windows. Pin the version. **Measure with `overflow-checks = true` in the release profile** — that is the shipping configuration per section 15, so measuring with checks off would produce a budget the game cannot honour. Record the delta between checked and unchecked builds anyway; it is useful to know what the safety costs.

**Step 1 — Machine hygiene, before any number is taken.** Record CPU model, core count, base and boost clocks, RAM, OS build. Disable turbo-variability as far as the machine allows or, failing that, run each configuration **three times and report the median of the three p99s**. Pin the process to a core. Close the browser. Note that Windows Defender real-time scanning and any ETW tracing add noise, and either exclude the directory or state that it was not excluded. A single-run p99 on a laptop on battery is not a measurement.

**Step 2 — Warm-up and steady state.** Run 2,000 ticks before recording anything, then measure over a **full 8-minute segment: 8 × 60 × 20 = 9,600 ticks** — the spec's longest segment, from the 3/5/8 ladder. Budget conclusions drawn from a 100-tick burst are worthless; allocator behaviour and the copy-on-write chunk store both change character over minutes.

**Step 3 — The gate configuration.** 4 seats, 300 units, 40 beacons (world totals), with a destruction stream at 20 explosions/s and a repath rate taken from G2 (or a `PLACEHOLDER` rate until G2 reports). Record:

- **total tick time: p50 / p90 / p99 / max, in ms** — G3′-a needs **p99 ≤ 25 ms**;
- **per-phase time**: p50 and p99 for each of the ten phases above, so the budget can be attributed;
- **mean tick and ticks/s** — G3′-b needs **≥80 ticks/s**;
- **headless speed multiple** = ticks/s ÷ 20;
- allocations per tick and peak RSS;
- the per-tick hash value stream, so this spike's runs are also determinism runs (free G4 evidence);
- cache-miss-ish proxies if available (instructions/tick, or simply time per unit), for the SoA layout decision.

**Step 4 — Sweep for the cost model.** Vary units over {50, 100, 200, 300, 450, 600} at fixed beacons, and beacons over {10, 20, 40, 80} at fixed units, and voxel edits over {0, 10, 20, 40}/s. Fit the per-entity marginal costs:

- **ns per unit per tick**;
- **ns per beacon per tick**;
- **ns per voxel edit**;
- **ns per decision tick per seat** (measured on the every-5th tick only);
- the fixed per-tick overhead (broadphase rebuild, hash, bookkeeping).

Record whether the unit cost is linear or super-linear — if the broadphase is doing its job it should be near-linear, and a super-linear curve is a finding about the broadphase, not about the budget.

**Step 5 — Derive the provisional per-map power budget.** This is the step the gate row calls out (*"The measurement sets the per-map power budget for the generator"*). Working:

1. Take the budget: 25 ms p99, minus a reserve for what the synthetic tick does not yet model (`PLACEHOLDER` reserve, recommended ≥40% at spike stage because a synthetic tick always flatters the real one).
2. Subtract fixed overhead and beacon/voxel costs at the gate configuration.
3. Divide the remainder by ns-per-unit-per-tick → **the affordable unit count, world-wide**.
4. Convert to power: section 8 ties units to kW, so affordable units × kW draw per unit (`PLACEHOLDER`, a Tuning value from section 19) = **total kW the map may supply**. That is the per-map power budget: the generator's vents and their output must sum below it.
5. Repeat for the ≥80 ticks/s mean constraint and take the **lower** of the two results.

Record the arithmetic explicitly, with every input named, so S2 can redo it with real numbers by substituting measurements for the spike's estimates.

**Step 6 — Segment-length check.** With the gate configuration, confirm the 8-minute segment completes with no drift or growth in tick cost from minute 1 to minute 8 (copy-on-write chunk growth and accumulating wrecks are the plausible culprits). Record tick p99 per minute. A rising curve is a leak, and it is better found here.

**Step 7 — Both first-class OSes.** Repeat steps 3–5 on Windows and Linux. **The budget is set by the slower of the two**, since both are first-class (section 15). macOS is a CI artefact only and its numbers are recorded as information.

**Step 8 — Write up**, including the explicit statement that these are synthetic numbers and that S2's exit supersedes them.

### Numbers to record (the results table)

| Number | Unit | Gate part |
|---|---|---|
| Tick p50 / p90 / **p99** / max at the gate configuration | ms | **G3′-a: p99 ≤ 25** |
| Mean tick, ticks/s, **speed multiple** | ×real time | **G3′-b: ≥ 4×** |
| Per-phase p50/p99, all ten phases | ms | attribution |
| Hash phase: full vs incremental | ms | the hashing decision |
| ns per unit / per beacon / per voxel edit / per decision tick per seat | ns | the cost model |
| Fixed per-tick overhead | ms | the cost model |
| Affordable unit count, and the derived **per-map kW budget** | count, kW | the generator's input |
| Tick p99 per minute over 8 minutes | ms | drift / leak check |
| Allocations per tick, peak RSS | — | allocator decision |
| Overflow-checks on vs off | % | cost of the safety rule |
| Windows vs Linux on every row | — | budget set by the slower |
| Per-seat reading (1,200 units / 160 beacons) | — | the open question in §1 |

## 4. CI hook

`.github/workflows/spike-g3.yml`. Small, and deliberately not a hard gate on hosted runners:

```yaml
name: spike-g3-tick-budget
on: [push, workflow_dispatch]
jobs:
  tick:
    strategy: { matrix: { os: [ubuntu-latest, windows-latest] } }
    runs-on: ${{ matrix.os }}
    steps:
      - run: cargo run --release --bin tickbench -- --seats 4 --units 300 --beacons 40 --ticks 9600 --json tick-${{ matrix.os }}.json
      # Assert against a LOOSE PLACEHOLDER threshold only: hosted vCPUs are slow and
      # noisy, so a 25 ms p99 asserted here would either flap or be meaningless.
      # The binding numbers are the dev-machine runs in the Results section.
      - run: cargo run --release --bin costmodel -- --sweep --json cost-${{ matrix.os }}.json
      - uses: actions/upload-artifact@PLACEHOLDER
```

What this hook is *for*: catching an order-of-magnitude regression in the synthetic tick between spike days, and producing the artefacts the write-up quotes. The shape it establishes — a tick benchmark emitting JSON, thresholds in one place — is what harness part 1 turns into the real per-stage perf check, and what section 17's *"machine-checkable definition of done"* leans on from S1 onward.

The spike also emits the per-tick hash stream from `tickbench`, so this workflow doubles as a second, independent determinism trace for G4 at no extra cost.

## 5. Platform risks: Windows/MSVC vs Linux

| Risk | Why it differs | How this spike handles it |
|---|---|---|
| **Measuring with overflow checks off** | Release builds normally disable them; the spec requires them on. A budget measured without them is 5–20% optimistic and the whole power budget inherits the error. | `overflow-checks = true` in the measured profile, always. The checked-vs-unchecked delta is recorded separately as information. |
| **Allocator** | The Windows default heap is materially slower than glibc's malloc for the many small allocations persistent structures (`imbl`) produce, so the same code yields two different budgets and the slower one governs. | Record allocations per tick; measure both OSes; record whether an alternative allocator closes the gap. Choosing one is a decision for the skeleton, but the spike must say whether it matters. |
| **Persistent/copy-on-write structures** | `imbl` and the 32³ copy-on-write chunk store allocate on write and hold structure alive; their cost profile changes over an 8-minute run and differs by allocator. | Step 6's per-minute tick p99 is exactly this check. A rising curve is reported as a finding, not smoothed away. |
| **Timer resolution and QPC** | `Instant` is QPC-backed on Windows and `CLOCK_MONOTONIC` on Linux; both are fine at millisecond scale, but Windows' default scheduler tick (~15.6 ms) distorts anything that sleeps. | The bench never sleeps and never yields: it runs ticks back to back and measures the loop. A "run at real time" mode is a separate, unmeasured code path. |
| **Frequency scaling and thermals** | An 8-minute run will downclock a laptop. The last minute is then slower than the first for reasons unrelated to the sim — which would be misread as the leak step 6 is hunting. | Record clocks before and after; three repetitions, median of p99s; if thermals dominate, run on a desktop and say so. |
| **MSVC vs LLVM codegen** | Rust uses LLVM on both, but the target ABI, the C runtime, and the standard library's platform layers differ; hot integer loops can genuinely differ by 10–20%. | Both OSes measured; the budget takes the slower. Never extrapolate one platform's number to the other. |
| **Wall-clock time inside sim code** | Forbidden by the lint set, and this spike is entirely about time. | `Instant` lives only in `src/bin/`. The tick function takes no clock and returns no duration; the harness times the call. |
| **`as` casts and float creep in a benchmark** | Statistics (percentiles, means, ns-per-entity fits) beg for floats and casts, and habit spreads. | Statistics live in the bench binary, outside the sim module, where floats are permitted for reporting. The tick itself stays integer. The spike states plainly which side of the wall each file is on — the same wall G1 has to draw for the mesher. |
| **Hash cost hidden by the allocator** | If the hash allocates a buffer per tick, its cost is really an allocator cost and will change with the allocator. | Hash into a reused buffer; record hash-phase allocations as zero (or explain). |
| **Hosted-runner noise** | Any p99 threshold asserted on shared vCPUs will either flap or be so loose it asserts nothing. | CI asserts a loose regression alarm only; binding numbers come from the dev machine, and the Results section records which machine each number came from. |

## 6. Fallback if it fails

The spec's fallback is **"Lower map power budget; shorter segments"**, and both levers are numbers rather than rewrites — which is the reason this gate is only a synthetic check in the spike.

1. **Lower the per-map power budget.** Step 5's arithmetic runs backwards: the affordable unit count falls out of the measured tick cost, and the generator's total vent supply is set below it. Section 8 already routes the unit ceiling through power (*"Beacon spam adds no power. Map generation keeps total supply within the per-map power budget set by G3′"*), and section 19 lists the per-map power budget as a Tuning value settled at S2's exit. So this fallback is the system working as designed, not a compromise.
2. **Shorten segments.** The 3/5/8 ladder becomes a shorter list. Note this interacts with the fun gate, whose own fallback ladder *starts* with "shorter default Push ladder with more rounds" — so if both gates point the same way, the change is doubly motivated and should be made once.
3. **A third lever the spec does not name but the cost model may hand us:** if the breakdown shows one phase dominating (the hash, the broadphase, the decision tick), fix that phase instead of cutting the budget. An incremental hash, a better broadphase, or a decision tick staggered across seats so only one seat's rules run on any given tick are all ordinary engineering, and the spike's per-phase numbers are what tells us whether that option exists. Only when no phase dominates does the budget itself have to move.

The gate cannot stop the project (section 16), and the real measurement is S2's exit. A spike failure means S2 starts with a lower provisional budget and a named hot phase to attack.

## 7. Estimate

**2 days**, in worktree A during week 3 of the 3-week budget. The shortest of the four, because it reuses G4's toy sim shape and because its binding measurement is at S2.

| Day | Work |
|---|---|
| 1 | SoA tables + the ten tick phases + broadphase + per-tick hash (full and incremental); the bench harness with per-phase timing |
| 2 | Gate-configuration run, the sweep and the cost-model fit, the power-budget arithmetic, both OSes, write-up |

If G4 has not finished when this starts, add half a day to stand up the fixed-point and hashing pieces from scratch.

## 8. The decision this spike must produce

> **The provisional per-map power budget, and the tick's cost model behind it** — specifically: (1) ns per unit per tick, per beacon per tick, per voxel edit and per decision tick per seat, with the fixed overhead, measured on both first-class platforms; (2) the kW figure the map generator may not exceed, with the arithmetic that produced it and the reserve it assumes, explicitly marked provisional until S2's exit; (3) whether the per-tick state hash is full or incremental — a determinism-contract decision, not a performance one; (4) the SoA table layout and the broadphase structure the skeleton should start from; (5) whether the 3/5/8 segment ladder is affordable as written.

Recorded in `docs/design/decisions-log.md` §2.7 and cross-referenced from section 19's Tuning row for kW. Item (3) is a contract file and needs owner approval.

## 9. Results

*(Empty until the spike runs. Every number here is synthetic and is superseded by S2's exit measurement.)*

- Machine: `PLACEHOLDER` (CPU, cores, clocks, RAM, OS build, on mains / desktop?) · Rust: `PLACEHOLDER`
- G3′-a tick p99 at 4 seats / 300 units / 40 beacons, Windows / Linux: `PLACEHOLDER`
- G3′-b ticks/s and speed multiple: `PLACEHOLDER`
- Dominant phase and its share: `PLACEHOLDER`
- Hash: full `PLACEHOLDER` ms vs incremental `PLACEHOLDER` ms
- Cost model: `PLACEHOLDER` ns/unit/tick, `PLACEHOLDER` ns/beacon/tick, `PLACEHOLDER` ns/voxel edit
- **Provisional per-map power budget: `PLACEHOLDER` kW** (reserve assumed: `PLACEHOLDER`%)
- 8-minute drift: `PLACEHOLDER`
- Verdict: `PLACEHOLDER` (go / which lever of the fallback)
