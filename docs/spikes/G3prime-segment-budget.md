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

**Verdict: go on the binding constraint, conditional on the other.** G3′-b — the
tighter of the two, as §1 predicted — passes with 15% of headroom. G3′-a fails at
an unbounded per-tick repath cap and passes at a cap of 16; the failure is a
readout of one constant nobody has measured, and the lever that fixes it is
neither of the two the spec's fallback names.

| Assertion | Budget | Measured | |
|---|---|---|---|
| **G3′-a** tick p99 at the gate configuration | ≤ 25 ms | **56.225 ms** at an unbounded repath cap | ❌ |
| **G3′-a** the same, at a per-tick repath cap of 16 | ≤ 25 ms | **16.465 ms** | ✅ |
| **G3′-b** mean tick / speed multiple | ≤ 12.5 ms / ≥ 4× | **10.620 ms / 4.708×** | ✅ |
| **G3′-c** load | 4 seats, 300 units, 40 beacons | as specified, world totals | ✅ |
| Scrub | — | **not here**; judged at S7 | — |

**Every number in this section is synthetic and is superseded by S2's exit
measurement** (section 16: *"the real measurement is S2's exit"*). It is worse
than that, and §9.2 says so with a number: **96.5% of the gate tick is a cost
this spike *charges* rather than measures**, and the single constant that decides
the G3′-a verdict is a `PLACEHOLDER`. Read §9.1 and §9.2 before quoting anything
below them.

### 9.1 Provenance

- **Machine.** Intel Core i7-9800X, 8C/16T, nominal 3.79 GHz · 31.7 GiB RAM ·
  **desktop on mains**. Windows 10 Pro 19045 (10.0.19045). Nothing else heavy
  was running. The spike directory was **not** excluded from Windows Defender
  real-time scanning, and no ETW tracing was disabled, so every number includes
  whatever those cost on this machine. This is stated rather than claimed
  otherwise, as plan §3 step 1 requires.
- **Clocks, recorded before, during and after every run** (`results/hygiene.json`).
  `Win32_Processor.CurrentClockSpeed` reports the nominal **3 792 MHz** on this
  box whatever the core is doing, so it carries no information; the figure with
  signal is the `\Processor Information(0,2)\% Processor Performance` counter for
  the pinned core, sampled every 40 ms while each run was in flight. Across the
  ten sampled runs the pinned core held **108.3–111.3% of nominal on the mean (≈ 4.11–
  4.22 GHz)**, with a per-sample min of 94.6% and a max of 112.7%. There is no
  downward trend within any run and none across the 78 minutes of measurement:
  this is a desktop that boosts and stays boosted, which is why plan §5's
  "frequency scaling and thermals" row does not bite here and why §9.10's drift
  numbers can be read as the sim's rather than the machine's.
- **Core pin, from outside the process.** Each run was started with
  `Start-Process -PassThru` and then pinned with `Process.ProcessorAffinity = 0x4`
  — core 2, the core G2 used — and the affinity was read back and recorded
  (`0x4` on every run). The worker thread additionally pins itself to the same
  core from inside (`--pin-core 2`), so the process mask and the thread mask
  agree; the few milliseconds between process creation and the external mask
  landing sit inside the 2 000-tick warm-up, never inside a measured segment.
- **A 6-second pinned busy loop precedes every launch.** Windows ramps the clock
  per core, and the harness calibrates its synthetic workload in the first
  ~120 ms of the process — on a core that has just been idle at ~1.19 GHz. The
  calibration is largely self-warming (it doubles the iteration count until one
  ramp lasts 20 ms and takes the median of five), and cold and pre-warmed both
  report 1 664–1 665 ps/iter here; what the pre-warm removes is the ramp inside
  the first seconds of the warm-up. It is machine hygiene done from outside, so
  the measured binary is untouched.
- **Toolchain.** Rust 1.98.1 `stable-x86_64-pc-windows-msvc` (rustc `48a229cea`,
  2026-09-01), pinned by `spikes/g3-tick-budget/rust-toolchain.toml`. One
  dependency, pinned exactly: `xxhash-rust =0.8.18` (`xxh3`), the same pin the G4
  spike carries, because the per-tick hash is the artefact both spikes produce.
  `Cargo.lock` is committed and every invocation passes `--locked`.
- **Profile.** `release` with **`overflow-checks = true`** (plan §3 step 0),
  `lto = "thin"`, `codegen-units = 1`. The binary self-reports the profile into
  every JSON, so an artefact carrying a budget can be shown to have come from the
  measured configuration. `release-unchecked` is used for the delta of §9.11 and
  for nothing else.
- **Segment.** 9 600 ticks — a full 8-minute segment, the longest of the 3/5/8
  ladder — after 2 000 ticks of warm-up. The sweep points are 2 400 ticks each
  after the same warm-up.
- **Repetition.** The gate, the cap-16 run and both destruction-off runs are the
  **median of three** whole runs (plan §3 step 1); every sweep point in
  `cost.json` is likewise the median of three. The incremental-hash run, the
  overflow-checks-off gate run and the three burst-rate runs are **single
  samples** and are marked as such wherever they are quoted.
- **Calibration.** 1 608–1 664 ps per iteration of the synthetic workload,
  measured afresh by each process — the constant that converts G2's measured
  nanoseconds into charged iterations. It is **outside hashed state on purpose**,
  which is what lets machines that calibrate differently still be compared on
  their hash streams (§9.13).
- **Gate configuration.** 4 seats, 300 units (75 a seat), 40 beacons (10 a seat),
  80 structures, 20 craters/s, full hash, per-tick repath cap unbounded unless a
  row says otherwise. Map 384 × 384 × 64 voxels, match seed
  `0x123456789ABCDEF0`.
- **Measured** 2026-09-14, 78 minutes of wall time over eleven runs. Raw artefacts
  in `spikes/g3-tick-budget/results/`, which is git-ignored on purpose
  (`spikes/README.md`): what survives them is this section.
- **Green before any number was taken.** `cargo test --release --locked`: **25
  tests, 0 failures** (21 in `tests/spike.rs`, 4 in `tests/wall.rs`).
  `cargo clippy --all-targets --release --locked -- -D warnings`: clean under the
  spike-local `clippy.toml`, whose header documents the arrangement — the library
  is sim code and stays under the product's determinism set (no floats, no `as`,
  no `HashMap`/`HashSet`, no wall clock, no recursion, every sort key ending in a
  unique id), with `std::time::Instant` confined to `src/bin/*.rs` and that
  confinement asserted by a test rather than promised in a comment, exactly as G2
  did it.
- **Reproduce**, from `spikes/g3-tick-budget` with the toolchain on `PATH`:

  ```sh
  export CARGO_TARGET_DIR=D:/build/g3-tickbudget   # PowerShell: $env:CARGO_TARGET_DIR = 'D:/build/g3-tickbudget'
  cargo build  --release                    --locked --bins
  cargo build  --profile release-unchecked  --locked --bins
  cargo test   --release --locked                      # 25 tests
  cargo clippy --all-targets --release --locked -- -D warnings

  # all eleven runs, with the clocks, the external pin and the working-set sampler
  powershell -NoProfile -ExecutionPolicy Bypass -File ./run-measurements.ps1
  ```

  The two runs the headline numbers come from, on their own:

  ```sh
  tickbench --seats 4 --units 300 --beacons 40 --ticks 9600 --warmup 2000 \
            --edits-per-s 20 --repeat 3 --hash full --pin-core 2 \
            --json results/tick.json --hashes results/hashes.txt
  costmodel --seats 4 --ticks-per-point 2400 --warmup 2000 --repeat 3 \
            --pin-core 2 --json results/cost.json
  ```

  The hygiene that cannot be done from inside a dependency-free benchmark — the
  process pin, the clocks, and peak **working set** — is done from PowerShell,
  the way G2 did it (G2-P3-pathing.md §9.9 item 1); `PeakWorkingSet64` reads 0
  after exit, so it must be sampled live:

  ```powershell
  $before = Get-CimInstance Win32_Processor | Select-Object CurrentClockSpeed, MaxClockSpeed
  $p = Start-Process -FilePath .\tickbench.exe -ArgumentList '--units','300' -PassThru -NoNewWindow
  $p.ProcessorAffinity = 0x4                      # core 2, from outside
  $peak = 0
  while (-not $p.HasExited) {
      $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 }
      (Get-Counter '\Processor Information(0,2)\% Processor Performance').CounterSamples[0].CookedValue
      Start-Sleep -Milliseconds 40
  }
  $after = Get-CimInstance Win32_Processor | Select-Object CurrentClockSpeed, MaxClockSpeed
  ```

- **Quote these numbers at the precision the spread supports.** Three runs of the
  identical binary and seed put the gate p99 between 55.07 and 56.46 ms — 2.5%
  — so it is "about 56 ms", never six decimals. §9.13 has the spread for every
  repeated configuration, and the tail moves far more than the p99 does.

### 9.2 Two health warnings, before the numbers

**(1) 96.5% of the gate tick is charged, not measured.** The `pathing` phase
stands in for G2 (plan §2: *"the spike substitutes a fixed per-unit path-follow
cost plus a configurable repath rate"*). Its real half — the integer path-follow
over the unit table, laying routes and turning headings through the 4 096-entry
angle table — is **0.095 ms**. Its charged half — a calibrated busy loop
reproducing G2's measured 0.77 ms repath (1.84 ms once in a hundred), 2.5 ms of
cluster repair per crater and 0.5 ms of CSR rebuild per tick — is **10.256 ms**.
`tick.json` marks the phase `"substituted": true` and carries both halves
separately; `ci/annotate.py` prints the substituted share next to the gate
verdict. **The tick minus that phase is 0.268 ms.** Any sentence of the form "the
sim costs 10.6 ms a tick" is wrong; the sim costs 0.27 ms a tick and G2's pathing
load costs 10.3 ms on top of it.

A caveat on the split itself: `real_ms` is the measured phase mean minus the
charged figure, i.e. a small difference of two large numbers. The charged loop
executes within about ±3% of what `iters × ps_per_iter` predicts, so `real_ms`
carries the whole of that residual and in two of the ten `tickbench` runs it came out
slightly negative (most negative: −0.069 ms at cap 16). Read the split as "the
real half is under 0.2 ms, inside the noise of the charged half", not as a
measurement of the path-follow cost.

**(2) The G3′-a verdict is a readout of one constant nobody has measured.** G2
reported the burst's magnitude (`repaths_per_tick_peak: 74`, a once-per-run
maximum) and nothing about its frequency. The spike's burst rate is therefore a
`PLACEHOLDER` (`Config::burst_one_in`, default 1-in-64), and the same 9 600-tick
run gives:

| `--burst-one-in` | p50 ms | p99 ms | max ms | mean ms | G3′-a (p99 ≤ 25) |
|---|---|---|---|---|---|
| 32 | 9.404 | **52.105** | 55.729 | 10.557 | FAIL |
| 64 *(default)* | 10.186 | **56.225** | 63.172 | 10.620 | FAIL |
| 128 | 10.290 | **12.115** | 64.885 | 10.664 | **PASS** |
| off | 10.729 | **12.529** | 19.474 | 10.803 | **PASS** |

The mean moves by 2.3% across the whole sweep and the p99 moves by a factor of
4.6, because the base demand is scaled by `p / (p + 7)` to hold the long-run mean
at G2's measured 9.45 repaths per crater whatever the rate is. So **G3′-a's
failure at an unbounded cap is a property of this constant, not of the sim**, and
the `max` column shows why: wherever a burst exists at all it costs 56–65 ms, at
every rate. All the rate settles is whether such a tick lands above or below the
99th percentile of 9 600 samples — at 1-in-64 about 150 ticks burst and only 96
sit above the p99 index, so the p99 *is* a burst tick; at 1-in-128 about 75 do,
and it is not. *(Single samples, `--repeat 1`.)*

> **PLACEHOLDER — repath burst frequency. Owner, with G2, at the S2 exit
> measurement.** Until it has a value, quote G3′-a as conditional.

### 9.3 The results table of §3 — every row

| Number | Unit | Measured | Gate part |
|---|---|---|---|
| Tick p50 / p90 / **p99** / max, gate, unbounded cap | ms | 10.186 / 10.637 / **56.225** / 63.172 | **G3′-a: p99 ≤ 25 → FAIL** |
| Tick p50 / p90 / **p99** / max, gate, **repath cap 16** | ms | 9.977 / 14.959 / **16.465** / 18.836 | **G3′-a → PASS** |
| **Mean tick**, ticks/s, **speed multiple** | ms, /s, × | **10.620**, 94.165, **4.708×** | **G3′-b: ≤ 12.5 ms, ≥ 4× → PASS** |
| …at repath cap 16 | ms, /s, × | 10.441, 95.774, 4.788× | G3′-b → PASS |
| …with the destruction stream off | ms, /s, × | 0.762, 1 312.8, 65.6× | — |
| Per-phase p50 / p99, all eleven phases | ms | §9.4 | attribution |
| Dominant phase and its share | — | `pathing` **97.4%** *(substituted)*; `broadphase` **85.7%** of what remains | attribution |
| Hash phase: full vs incremental | ms | 0.0192 vs 0.0166; 8.000 vs 7.641 tables; 63 892 vs 59 757 bytes | the hashing decision |
| ns per unit per tick (sim's own, `a0`) | ns | **1 022** | cost model |
| ns per unit per tick at 20 craters/s | ns | 25 710 | cost model |
| ns per beacon per tick | ns | **unresolved**, bracket [231, 3 159] | cost model |
| ns per voxel edit — the chunk store *(measured)* | ns | **6 960** | cost model |
| ns per crater — G2's pathing consequence *(substituted)* | ns | 9 544 516 | cost model |
| ns per decision tick per seat | ns | **1 041** | cost model |
| Fixed per-tick overhead at 40 beacons | ms | **0.4746** (= 0.5000 G2 CSR − 0.0254 sim's own) | cost model |
| Affordable unit count, world-wide | count | **190** at 20 craters/s | the generator's input |
| **Provisional per-map power budget** | kW | **190 kW** | the generator's input |
| Tick p50 / mean per minute over 8 minutes | ms | §9.10; drift −7‰ on the mean, 0‰ on the p50 | drift / leak check |
| Allocations per tick | — | **0** | allocator decision |
| Peak live heap / peak working set | MB / KiB | 10.80 MB / 15 344 KiB (internal), 15 336 KiB (external) | allocator decision |
| Overflow-checks on vs off | % | **+1.45%** on the quiet tick, **+2.90%** on its non-pathing part | cost of the safety rule |
| Windows vs Linux on every row | — | **pending CI** — see §9.14 | budget set by the slower |
| Per-seat reading (1 200 units / 160 beacons) | ms | **33.586 mean / 215.863 p99** | the open question in §1 |

**G3′-b is the binding constraint, and it passes with 15% of headroom** (10.620 ms
against 12.5). G3′-a fails at an unbounded cap and passes at a cap of 16 for no
measurable cost on the mean (10.441 ms against 10.620 in `tickbench`; 10.577
against 10.559 in `costmodel` — the two disagree on the sign of a 0.2% effect,
which is the honest way to say it is free). Note that the per-tick repath cap is
**not** one of the two fallback levers §6 names: it is the third lever, the one
§6's own last paragraph anticipates — *"if the breakdown shows one phase
dominating… fix that phase instead of cutting the budget"* — and decisions-log
item 60 already defines it.

Both gate runs serve the same **89 226 repaths** over the segment and produce the
same per-tick hash stream: the cap changes *when* work is done, not *whether*.

### 9.4 Per-phase attribution (gate configuration, unbounded cap, 9 600 ticks)

| Phase | share | p50 ms | p99 ms | mean ms | n |
|---|---|---|---|---|---|
| `broadphase` | 2.1% | 0.2219 | 0.3828 | 0.23011 | 9 600 |
| `programs` | 0.0% | 0.0024 | 0.0040 | 0.00246 | 9 600 |
| **`pathing`** *(SUBSTITUTED)* | **97.4%** | 9.9307 | 55.9589 | **10.35111** | 9 600 |
| `movement` | 0.0% | 0.0021 | 0.0090 | 0.00289 | 9 600 |
| `combat` | 0.0% | 0.0020 | 0.0043 | 0.00212 | 9 600 |
| `kill_credit` | 0.0% | 0.0007 | 0.0020 | 0.00078 | 9 600 |
| `power` | 0.0% | 0.0014 | 0.0034 | 0.00159 | 9 600 |
| `quartermaster` | 0.0% | 0.0011 | 0.0023 | 0.00116 | 9 600 |
| `decision` | 0.0% | 0.0041 | 0.0073 | 0.00430 | 1 920 |
| `voxels` | 0.0% | 0.0068 | 0.0176 | 0.00728 | 9 600 |
| `hash` | 0.1% | 0.0178 | 0.0345 | 0.01919 | 9 600 |

**Eleven phases, not the ten of plan §2's table:** `pathing` is added between
`programs` and `movement` to stand in for G2, which the plan's "not in the toy"
paragraph says the spike substitutes rather than builds. The declared order is a
contract in the same sense the table order in `hash.rs` is — moving a phase moves
every hash.

`decision` runs on one tick in five and is sampled only on those; its **share** is
computed from its total over the whole run, so the eleven shares still describe
one tick. Harness bookkeeping (the sample push, the counter merge, the checksum
xor) is **outside** every timed span; the twenty-two clock reads that remain
inside the tick cost **791 ns** and are reported, not subtracted. The tick total
is exactly the sum of the eleven phase samples, so the table accounts for all of
it.

**The dominant phase is the substitute, which is why the interesting table is the
one with the destruction stream off** — the only configuration in which the sim is
visible. The same eleven phases then give a mean tick of **0.762 ms** (p50 0.767,
p99 1.039, max 1.163; 65.6× real time), and the shape is completely different:

| Phase | share | mean ms |
|---|---|---|
| `pathing` *(0.5000 of it is G2's CSR rebuild, charged unconditionally)* | 68.2% | 0.52008 |
| `broadphase` | 28.1% | 0.21466 |
| `hash` | 2.2% | 0.01732 |
| everything else (eight phases) | 1.3% | 0.00968 |

**The finding to carry forward: outside the pathing substitute, the broadphase is
the tick.** At the gate configuration it is 0.230 ms of the 0.268 ms that is not
the substitute — **85.7%**; with destruction off it is 0.215 of 0.242 —
**88.8%** — at 300 units, and §9.6 shows it growing faster than linearly from
there.

### 9.5 Full versus incremental hashing — plan §8 item (3)

*Single samples (`--repeat 1`) at the gate configuration, 9 600 ticks. The
byte and table counts are exact and deterministic; the millisecond column is not,
and the spread matters — see below.*

| | tables re-encoded / tick | bytes encoded / tick | `hash` phase mean | digest |
|---|---|---|---|---|
| full | **8.000** of 8 | 63 892 | 0.01919 ms *(0.01776 ms in the single-sample control run)* | `44217aea12b1af6a` |
| incremental | **7.641** of 8 | 59 757 | 0.01665 ms | `44217aea12b1af6a` |

Incremental encodes **6.5% fewer bytes** for the same 64-bit value on every tick
(`tests/spike.rs` pins that identity, and CI `cmp`s the two streams over a whole
segment). On time it is **6–13% cheaper** depending on which full-hash run it is
compared against — the `hash` phase's own mean ranged 0.0177–0.0192 ms across the
six full-hash gate runs taken today, so **the saving is inside the run-to-run
spread of the thing being saved.**

**Read the 7.641, not the percentage.** At whole-table granularity an incremental
scheme has almost nothing to skip in a live tick: `Units` (60% of the encoding)
moves every tick because every unit moves, `Header` carries the tick number,
`Beacons` accrue, `Seats` settle cash, and at 20 craters/s a crater lands on every
tick so `Voxels` is dirty too. What incremental buys is the occasional clean
`Credits`, `Projectiles` or `Structures` table. With destruction off, `Voxels`
goes clean permanently and the saving is larger — which is exactly the case the
product does not run in.

**Recommendation: take the full hash.** Not because the two cost the same — they
do not — but because the saving is on the order of **0.0011–0.0025 ms a tick:
0.02% of the gate tick, and under 1% of the part of it that is not the
substitute**, and smaller than the phase's own noise. The incremental scheme buys
that with a dirty flag on every write site in the sim, which is a permanent
correctness obligation on the determinism artefact itself — a missed flag is a
*wrong* hash, not a slow one. **This is a contract decision (AGENTS.md §5) and is
the owner's, not the spike's.**

Related, and measured so that it is a number rather than an assertion: re-hashing
the 9 437 184-byte chunk store instead of its per-chunk digests costs **0.834 ms**
(0.754–1.155 ms across the eleven runs) — about **43× the whole eight-table hash
phase**, and about **three times the entire non-pathing tick**. Hashing digests is
not an optimisation to revisit.

### 9.6 The cost model — plan §3 step 4

Sweeps: 30 points, each the **median of three** 2 400-tick runs after 2 000 ticks
of warm-up, pinned to core 2, in the checked profile (`cost.json` records the
profile and the pin, so the artefact carrying the budget can be shown to have come
from the measured configuration). **The `costmodel` gate point reconciles with
`tickbench`'s to 2.6% on the mean (10.346 vs 10.620 ms) and 2.6% on the p99 (54.78
vs 56.23 ms)** — the same order as the ±3% calibration residual of §9.2, and the
right precision to quote the model at.

**The fitted model** is `mean_tick = fixed + a0·units + β·beacons + c0·edits/s +
d·units·edits/s`. The product term is not optional: repath demand is G2's 9.45 per
crater *at 300 units* and scales with the unit count, so a model without it counts
that product twice and drives the fixed term negative.

| Quantity | Value | Read this with it |
|---|---|---|
| **ns per unit per tick** (`a0`, destruction off — the sim's own) | **1 022** | r² 0.992, but the fit is **not** the right summary: see the shape finding below |
| ns per unit per tick at 20 craters/s | 25 710 | r² 1.000 — 96% of it is G2's charge, and linear by construction |
| ns per unit per edit/s (`d`, G2's repath load) | 1 234 | |
| **ns per beacon per tick** (`β`) | **unresolved: 0.23–3.16 µs** | OLS says 701 ns at r² 0.73 over four points. The series is monotone and strictly increasing but strongly **saturating** — 3.16 µs/beacon from 10→20, 0.84 from 20→40, 0.23 from 40→80 — which is a real effect, not noise: a denser static population makes each nearest-enemy ring search terminate sooner. A single slope does not describe it. **The term cancels out of the budget arithmetic** (it sits inside the fitted intercept and is not added again), so nothing downstream depends on resolving it. |
| **ns per crater — G2's pathing consequence** *(SUBSTITUTED)* | **9 544 516** (9.54 ms) at 300 units | = 2 138 022 unit-free (cluster repair) + 24 688 per unit (repaths). This is what a crater *costs the pathfinder*, not what it costs to apply. |
| **ns per voxel edit — the chunk store** *(MEASURED)* | **6 960** (6.96 µs) | the `voxels` phase at one crater a tick. **1 371× smaller** than the line above. A reader who takes "ns per voxel edit" for the copy-on-write store and reaches for the pathing number is wrong by three orders of magnitude, which is why `cost.json` names the two apart. |
| ns per decision tick per seat | 1 041 | on the every-fifth tick, four seats |
| **fixed per-tick term at 40 beacons** | **0.4746 ms** | = **0.5000 ms of G2's CSR rebuild**, charged on every tick whether or not anything was destroyed, **− 0.0254 ms** left over. |

**The fixed term is not the sim's overhead.** Once G2's unconditional CSR rebuild
is subtracted, the sim's own fixed cost fits *slightly negative* — i.e. it is below
this fit's noise floor, and the measured non-pathing phases at the gate sum to
only 0.268 ms in total. The plan's cost-model bullet calls this quantity "the
fixed per-tick overhead (broadphase rebuild, hash, bookkeeping)"; 105% of it is a
G2 constant, and **S2 redoing this arithmetic against real code must not add the
CSR rebuild a second time**. Worth raising with G2 separately:
`csr_rebuild_ms_per_tick` was measured under a 20 craters/s stream, and charging it
at 0 craters/s is what makes this spike's quiet baseline three times the sim's own
cost.

**Unit cost is SUPER-LINEAR, and the broadphase is why.** The plan's step 4 asks
the question about the broadphase, so it has to be fitted where the broadphase is
visible — the destruction-off sweep — and not on the 20-craters/s total, which is
96–98% a term that is linear in units by construction and can only ever answer
"linear".

| units | 50 | 100 | 200 | 300 | 450 | 600 |
|---|---|---|---|---|---|---|
| tick mean, destruction off (ms) | 0.5399 | 0.5807 | 0.6780 | 0.7632 | 0.9086 | 1.1149 |
| `broadphase` phase (ms) | 0.0255 | 0.0597 | 0.1379 | 0.2181 | 0.3495 | 0.5255 |
| `hash` phase (ms) | 0.0051 | 0.0075 | 0.0135 | 0.0184 | 0.0251 | 0.0338 |

| fit | quadratic share at 600 units | marginal ns/unit across the sweep |
|---|---|---|
| **`broadphase` phase, destruction off** | **+33.9%** | 683 → 782 → 802 → 876 → **1 174** (+72%) |
| whole tick, destruction off | **+17.3%** | 815 → 973 → 853 → 969 → **1 375** (+69%) |
| whole tick, 20 craters/s *(the substitute — a control, not a finding)* | +4.1% | linear, as designed |

The linear fit's residuals on the quiet total are **+14.2, +3.8, −1.1, −18.0,
−25.9, +27.0 µs** — negative through the middle of the range and strongly positive
at both ends, which is curvature rather than scatter, and is exactly what
r² = 0.992 hides. The mechanism is not mysterious: the grid is 16 × 16 = 256 cells
of 24 voxels over a 384² footprint, so at 600 units a ring search's 3 × 3
neighbourhood holds about 21 candidates against about 2 at 50 units, and the
per-query candidate count grows with density. The gate run confirms the mechanism
in absolute terms: 5 756 542 queries scanned **74 296 970 candidates** over the
segment, 12.9 per query.

> **The finding for the skeleton: the uniform grid is fine at the gate load
> (0.22 ms at 300 units, 28% of the quiet tick) and is the first thing to outgrow
> it.** At 600 units it is 47% of the quiet tick and still rising per unit. Keep
> the SoA tables and the CSR grid — rebuild is O(items + cells) and allocates
> nothing after construction — but expect the *query* to need a cell size tied to
> density, or a coarser two-level grid, before the unit ceiling doubles. That is a
> broadphase task, not a budget cut.

### 9.7 Step 5 — the provisional per-map power budget, with every input named

This is the step the gate row calls out (*"The measurement sets the per-map power
budget for the generator"*). Every input is named so that S2 can redo the
arithmetic by substituting measurements for the spike's estimates.

| Step | Input | Value |
|---|---|---|
| 1 | G3′-a budget | 25 ms p99 (50% of a 50 ms tick) |
| 1 | G3′-b budget | 12.5 ms mean (≥ 4× real time) |
| 1 | **reserve — `PLACEHOLDER`, owner at S2's exit** | **40%** (plan §3 step 5: *"≥ 40% at spike stage because a synthetic tick always flatters the real one"*) |
| 1 | → effective p99 / mean | 15.000 ms / **7.500 ms** |
| 2 | fixed per-tick term at 40 beacons (incl. G2's 0.5 ms CSR rebuild) | 0.47461 ms |
| 2 | unit-free crater cost at 20 craters/s (G2's cluster repair) | 2.13802 ms |
| 2 | beacon term | inside the fixed term; **not added again** |
| 3 | cost of one unit at 20 craters/s (`a0 + d·e`) | 25 710.5 ns = **0.025711 ms** |
| 3 | → **affordable units, world-wide** | **(7.500 − 0.47461 − 2.13802) / 0.025711 = 190** |
| 4 | **kW per unit — `PLACEHOLDER`, Tuning, section 19** | **1 kW** (the toy's `UNIT_KW`, standing in for the real value) |
| 4 | → **PROVISIONAL PER-MAP POWER BUDGET** | **190 kW** |
| 5 | the p99 branch solves for the cap, not the count | max sustainable per-tick repath cap **15** (floor 3.169 ms, 0.77 ms a repath from G2) |

Step 5 of the plan says to *"repeat for the ≥ 80 ticks/s mean constraint and take
the lower of the two results"*. It does not produce a second unit count here, and
the reason is worth stating rather than hiding: **the p99 constraint is not a
unit-count constraint at all.** The tail of this distribution is a burst of
repaths landing on one tick, and the per-tick cap is an absolute ceiling on how
much of that burst any one tick may carry — it does not depend on the unit count.
So the mean branch fixes the count (190) and the p99 branch fixes the cap (15).

The same arithmetic at other destruction rates, because the answer is dominated by
the crater stream and not by the units: **6 873 units at 0 craters/s, 446 at 10,
190 at 20, 55 at 40.** A unit costs **25× more** with the gate's destruction stream
running than without it, and essentially all of that is G2's repath load. **So the
"per-map power budget" this spike produces is really a statement about the
destruction rate**, and S2 should set the two together.

### 9.8 The repath-cap sweep

2 400 ticks a point, median of three, at the gate configuration.

| cap | mean ms | p99 ms | mean backlog | backlog growth | served | verdict |
|---|---|---|---|---|---|---|
| 8 | 9.335 | 10.993 | **4 973.6** | **+3 092** | 0.861 | **UNSUSTAINABLE** — the cheapest row in the table, and not a configuration: the queue never drains and 14% of the modelled work is dropped for ever |
| **16** | 10.577 | **16.732** | 2.85 | 0 | 1.000 | **sustainable; passes G3′-a and G3′-b** |
| 32 | 10.469 | 28.061 | 0.67 | 0 | 1.000 | sustainable; **fails G3′-a** (28.1 > 25) |
| unbounded | 10.559 | 55.868 | 0.00 | 0 | 1.000 | sustainable; fails G3′-a |

The **stability floor is 9.45** — mean demand is G2's 9.45 repaths per crater at
one crater a tick — and the ceiling is **15.4**, from the p99 arithmetic of §9.7
*after* its 40% reserve. Measured against the raw 25 ms gate rather than the
reserved 15 ms, a cap of 16 passes with 33% to spare and 32 fails; the two
readings bracket the answer between 15 and 16. That is a narrow window, and it is
narrow because of the burst constant of §9.2: **recommend a cap of 15–16, and
re-derive it at S2's exit once the burst frequency is a measurement.**
`cost.json` carries `repath_backlog_mean`, `repath_backlog_growth` and
`sustainable` for every sweep point precisely so that the cheapest row in the
table cannot be read as the best one.

**What the cap was and was not measured doing.** The charged half of the pathing
substitute has no subject: it charges a cost without dispatching work to a row. So
the spike measures **what a cap costs, not what a cap picks** — decisions-log item
60's round robin (seat, then beacon, then unit) is *not* exercised here, and
harness part 1 must not read it as though it were.

### 9.9 Plan §1's open question — world totals or per seat?

The spike measured **world totals** as the primary figure and the per-seat reading
as a secondary stress number, so the answer is available either way.

| | world totals (300 / 40) | per seat (1 200 / 160) |
|---|---|---|
| mean tick | 10.620 ms — **G3′-b PASS** | **33.586 ms — 2.7× over** |
| p99 tick | 56.225 ms (16.465 at cap 16) | **215.863 ms — 8.6× over** |
| `broadphase` phase | 0.214 ms | **1.542 ms — 7.2×** |
| `pathing` phase *(substituted)* | 10.097 ms | 31.924 ms |
| `hash` phase | 0.018 ms | 0.072 ms |

**If the gate row means per-seat counts, this configuration does not fit, and the
answer is not a tuning change.** Note that the per-seat broadphase grows 7.2× for a
4× unit count — the super-linearity of §9.6, showing up where it hurts. The owner
settles which the row means; the spec does not say.

### 9.10 Drift over the 8-minute segment — plan §3 step 6

**The p99 series cannot do this job at the gate configuration and must not be
quoted for it.** It is bimodal: the repath burst puts ~1.5% of ticks five times
above the rest, so a 1 200-tick bucket's 99th percentile reports how many bursts
that minute drew, not how expensive that minute was.

| minute | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | drift |
|---|---|---|---|---|---|---|---|---|---|
| p50 ms | 10.186 | 10.186 | 10.178 | 10.186 | 10.183 | 10.188 | 10.190 | 10.189 | **0‰** |
| mean ms | 10.706 | 10.508 | 10.635 | 10.453 | 10.518 | 10.699 | 10.810 | 10.628 | **−7‰** |
| p99 ms *(bimodal — do not read)* | 56.76 | 55.66 | 56.62 | 13.51 | 13.40 | 56.53 | 56.68 | 55.64 | (−19‰, luck) |

With destruction off, where the copy-on-write chunk store and the accumulating
wrecks would show up unmasked: p50 0.7572 → 0.7699 ms (**+16‰**), mean 0.7563 →
0.7650 ms (**+11‰**), rising over minutes 1–4 and then flat. **No leak.** A 1–2%
wobble over eight minutes with turbo enabled is scheduling and clock residue, not
the chunk store — and the clock series of §9.1 backs that up: the pinned core's
performance counter shows no downward trend inside any run.

`tick.json` carries `per_minute_p50_ms` and `per_minute_mean_ms` alongside
`per_minute_p99_ms`, and `drift_permille_first_to_last_minute` is computed from
the mean series.

**The 3/5/8 segment ladder is therefore affordable as written** at this load: an
8-minute segment costs the same in its last minute as in its first, on the mean
and on the p50, at both destruction rates.

### 9.11 Allocation, memory, and the overflow-checks delta

**Allocations: zero.** `allocs_per_tick_milli = 0` on every run — the measured tick
allocates nothing at all, at the gate configuration and with destruction off.
Peak live heap **10.80 MB** at the gate (12.80 MB in the runs that took the larger
scratch path), peak working set **15 344 KiB** read by the process itself and
**15 336 KiB** sampled from outside — 8 KiB apart, which is the 40 ms polling
granularity and not a discrepancy. Plan §5's allocator row therefore has nothing
to weigh on this side of the wall: there is no small-allocation traffic for
Windows' heap to be slower at, and the copy-on-write chunk store's `Arc` copies
settle during warm-up.

**The memory sampler is invisible in the distribution.** A control run of the gate
configuration taken *without* the PowerShell poller gives p99 56.537 ms and mean
10.690 ms against 56.225 and 10.620 with it — inside the 2.5% run-to-run spread of
§9.13. The G2 precaution (use the sampled run only for the memory row) is not
needed here, and both numbers are published so that the claim can be checked.

**Overflow checks (plan §3 step 0, §5 row 1).** The gate configuration **cannot**
measure this: at 20 craters/s ~96% of the tick is a calibrated busy loop pinned to
G2's milliseconds, so it is profile-neutral by construction. The destruction-off
pair is the one that can see it.

| | checked | unchecked | delta |
|---|---|---|---|
| mean tick, destruction off (median of 3) | 0.761739 ms | 0.750840 ms | **+1.45%** |
| …minus the `pathing` phase | 0.241663 ms | 0.234858 ms | **+2.90%** |
| mean tick, gate (20 craters/s) | 10.619592 ms | 10.476878 ms | +1.36% — diluted, do not quote |

So the safety rule costs about **3% of the sim's own integer work** here, well
inside plan §5's "5–20% optimistic" worry. **Keep the checks on.** *(The unchecked
gate run is a single sample against a median of three; the destruction-off pair is
median of three on both sides.)*

### 9.12 Odds and ends worth carrying to S2

- **`kill_credit` is barely exercised, and its headline distribution is the empty
  loop.** 37 deaths against 41 036 attacks over the segment: the toy's support
  program regenerates 1 HP a tick, which outruns 7 damage per 12-tick cooldown.
  The settlement path is therefore published separately — on the 36 sampled ticks
  that settled a death it costs p50 0.0010 / p99 0.0022 / mean 0.00104 ms, against
  0.00078 ms for the phase as a whole. Spec section 15 names those counters as
  hashed state, so S2 should re-measure the phase at a representative death rate
  rather than inherit this number.
- **Decision tick**: 0.00430 ms for four seats on every fifth tick, i.e. ~1.07 µs a
  seat, over **4 563 500 rule evaluations** in the segment. Section 16's P1 row has
  room.
- **The `voxels` phase**: 0.00728 ms a tick at one crater a tick — the
  copy-on-write chunk store's *own* cost of a crater, ~7 µs, three orders of
  magnitude below the pathing consequence of the same crater (§9.6). 466 971
  voxels were removed and 10 665 chunk digests refreshed over the segment. Do not
  confuse the two costs; `cost.json` names them apart deliberately.
- **Brownouts fire**: 11 711 over the segment, so the `power` phase measured is the
  phase that will run, including the per-tick re-sort of the shed order from the
  live priorities.
- **The whole-store rehash is not in the tick, and that is a design choice with a
  number on it**: 0.834 ms against a 0.019 ms hash phase (§9.5).

### 9.13 The spread, and the hash-stream identity

Three runs of the identical binary and seed, per configuration:

| configuration | p99 of run 1 / 2 / 3 (sorted) | spread | digest |
|---|---|---|---|
| gate, unbounded cap | 55.067 / 56.225 / 56.465 ms | 2.5% | `44217aea12b1af6a` |
| gate, repath cap 16 | 16.420 / 16.465 / 16.472 ms | 0.3% | `44217aea12b1af6a` |
| destruction off, checked | 0.9957 / 1.0389 / 1.0633 ms | 6.5% | `a700eff9dd67aec2` |
| destruction off, unchecked | 0.9389 / 0.9619 / 0.9816 ms | 4.4% | `a700eff9dd67aec2` |

The smaller the number, the larger the relative spread — so quote the gate p99 to
three digits and the quiet tick to two, and never quote a `max` as if it were a
property of the code (the gate's max ranged 59.3–63.2 ms across today's four unbounded-cap
runs at the default burst rate).

**The cross-configuration determinism check ran today and is free.** The four hash
streams produced locally — full hash, incremental hash, repath cap 16, and the
overflow-checks-off build — are **byte-identical over all 9 600 ticks**: 163 200
bytes each, LF endings only, MD5 `46e8b666d93245da76d3942d3dc535b6`, first line
`364cdd7640f9c900`, last `860e4375bfab505c`, folded digest `44217aea12b1af6a`.
That is the same claim `ci/spike-g3.yml`'s `hash-identity` job makes across
operating systems, made here across build configurations. The repath cap, the
burst rate and the clock calibration are all deliberately outside hashed state and
`tests/spike.rs` pins that, so the comparison is a controlled experiment rather
than a coincidence. This is the free G4 evidence plan §4 promised.

### 9.14 Windows versus Linux — plan §3 step 7, and why this row is empty

**Pending CI.** `ci/spike-g3.yml` is written and sitting in the spike directory but
has not been installed as `.github/workflows/spike-g3.yml`, so no Linux or macOS
run exists. Those numbers are **not guessed here**: the Linux column of the
results table, and the Linux and macOS halves of the three-way hash-stream
comparison, are recorded as *pending CI* and will be filled from the hosted
runners' own artefacts, with the hosted-runner caveat plan §5's last row already
states (shared vCPUs; the CI thresholds are loose regression alarms, never the
gate).

More important than the gap: **the gate-configuration total is the wrong figure to
compare across platforms, and comparing it would produce a false reassurance.**
The pathing charge is G2's measured nanoseconds divided by the host's own measured
picoseconds-per-iteration, so it reproduces G2's milliseconds on *every* machine by
construction. A runner 20% slower at hot integer loops still reports ~10 ms of
pathing, and the tick totals agree for a reason that has nothing to do with the two
platforms. The figure that does vary is emitted as `tick_minus_pathing_mean_ms` —
**0.268 ms** at the gate here, **0.242 ms** with destruction off — and
`ci/spike-g3.yml` carries a destruction-off leg so the comparison has something
with signal in it. Compare those columns; ignore the totals. The workflow header
says all of this out loud.

### 9.15 What could not be measured, and what was measured differently

Stated rather than buried, because each is a place where the number above does not
mean quite what §3 asked for.

1. **Linux and macOS were not run** (§9.14). Step 7's "the budget is set by the
   slower of the two" is therefore unexecuted, and the budget in §9.7 is a Windows
   number.
2. **The `pathing` phase is charged, not measured** (§9.2). 96.5% of the gate tick
   is a busy loop calibrated to reproduce G2's milliseconds. The spike's job was to
   find out whether *G2's load plus a sim* fits in a tick, and that is what it
   answers; it is not a measurement of a pathfinder, and S2's real one may differ
   in both directions.
3. **The repath cap was measured for what it costs, not for what it picks**
   (§9.8). Decisions-log item 60's round robin over (seat, beacon, unit) is not
   exercised, because the charged half of the substitute has no subject.
4. **The repath burst frequency is a free parameter, not a measurement** (§9.2),
   and it is the parameter the G3′-a verdict reads out.
5. **Peak RSS is reported twice, from inside and from outside.** The harness reads
   its own working set through a hand-declared `K32GetProcessMemoryInfo` (no
   dependency), and PowerShell polls `Process.PeakWorkingSet64` every 40 ms from
   outside. The two agree to 8 KiB, which is the strongest thing either could say
   about the other. G2 could not do the first half and took only the second.
6. **The beacon term is unresolved** and is published as a bracket, [231, 3 159]
   ns/beacon, because the four-point series saturates by 13.7× across its range.
   Nothing downstream depends on it: it lives inside the fitted intercept and is
   never added again.
7. **`kill_credit` ran on 37 deaths** in 9 600 ticks (§9.12), so its headline
   distribution is the cost of *not* settling anything. The settled-tick subset is
   published separately and is the number S2 should re-measure.
8. **The "fixed per-tick overhead" is 105% a G2 constant** (§9.6). The plan asked
   for "broadphase rebuild, hash, bookkeeping"; what the fit returns is G2's
   unconditional CSR rebuild with the sim's own overhead lost in the noise
   underneath it. S2 must not charge the CSR rebuild twice.
9. **`real_ms` is a difference of two large numbers** and carries the whole ±3%
   calibration residual; it went slightly negative in two of the ten `tickbench` runs
   (§9.2). Treat it as a bound, not a measurement.
10. **The super-linearity finding comes from the destruction-off sweep**, not from
    the gate configuration, because the gate's unit-cost curve is 96% a term that
    is linear in units by construction and could only ever answer "linear" (§9.6).
11. **The chunk store is hashed by per-chunk digests, not by its bytes**, in both
    hash modes — so "full" does not mean "hashes everything". The exclusion is
    published as a number (0.834 ms for a whole-store rehash) rather than as an
    assertion (§9.5).
12. **No snapshot or restore round-trip was taken.** That is G4's subject; this
    spike takes no `postcard`/`serde` dependency and does not serialise state.
13. **One map, one seed, one shape.** 384 × 384 × 64 voxels, seed
    `0x123456789ABCDEF0`, 16 × 16 broadphase cells. The broadphase's density story
    would read differently on a larger footprint at the same unit count, and the
    spike has no evidence about that.
14. **The clocks were recorded, not controlled.** Turbo was left enabled; the
    mitigation is the plan's own — three repetitions and the median of the p99s —
    plus a per-run clock trace showing the pinned core never left 108–112% of
    nominal. Nothing was excluded from Windows Defender.

### 9.16 Decision candidates (plan §8, plus §1's open question)

§8 asks this spike for the provisional per-map power budget and the five parts of
the cost model it rests on; §1 leaves one question open that changes the answer by
4×. Each item below gives the candidates the measurement actually leaves open, a
recommendation, and what the alternatives cost, so the owner can be interviewed one
item at a time. **Nothing here has been written to `docs/design/decisions-log.md`
§2.7 — that file is a contract file (AGENTS.md §5) and the entry is the owner's to
make.** Item (3) is part of the determinism contract and needs owner approval
alongside G4's.

**(0) §1's open question: are "300 units, 40 beacons" world totals or per seat?**

- **(Recommended) World totals** — 300 units and 40 beacons across 4 seats, i.e. 75
  and 10 each. It is the reading the spike measured as primary, it passes G3′-b with
  15% of headroom, and it is the only reading under which the spike produces a
  usable power budget at all.
  *Downside:* it is the generous-to-the-engine reading, and it authorises a
  three-seat v1 match of only ~100 units a seat. If a later design wants a seat to
  field 300, the budget has to be re-derived, not scaled — §9.9 shows the
  broadphase grows 7.2× for a 4× unit count.
- *Alternative — per-seat counts (1 200 units / 160 beacons world-wide).*
  *Downside:* measured at **33.586 ms mean and 215.863 ms p99** — 2.7× over G3′-b
  and 8.6× over G3′-a. No tuning value recovers that; it is a different engine, and
  the gate would be failed rather than conditional.
- *Alternative — leave it open until S2.*
  *Downside:* the per-map power budget is the map generator's input and the
  generator is built at the walking skeleton, so "open" means the generator is
  written against a number that may move by a factor of four.

**(1) The cost model — ns per unit, per beacon, per voxel edit, per decision tick,
and the fixed overhead.**

- **(Recommended) Adopt the five numbers as the skeleton's provisional model**, in
  the shape §9.6 publishes them: `a0 = 1 022` ns/unit/tick for the sim's own work,
  `d = 1 234` ns per unit per edit/s for G2's repath load, `6 960` ns per voxel edit
  for the chunk store, `1 041` ns per decision tick per seat, and a fixed term of
  `0.4746` ms of which `0.5000` is G2's CSR rebuild. Publish the beacon term as a
  bracket, and keep the chunk-store cost and the pathing consequence of a crater
  under different names.
  *Downside:* `a0` is a linear fit over a series that is measurably super-linear, so
  it under-states the marginal unit above 300 and over-states it below. It is a
  budget input, not a law.
- *Alternative — publish only the measured phase means and no fitted model.*
  *Downside:* step 5 then has no arithmetic, and the per-map power budget becomes a
  guess rather than a derivation. The plan explicitly asks for the marginal costs.
- *Alternative — fit a quadratic unit term and carry it forward.*
  *Downside:* it would be over-fitting a six-point sweep of a *synthetic* broadphase
  whose cell size and map footprint the real one need not share. The shape is a
  finding about the grid (§9.6); the coefficient is not worth carrying.

**(2) The provisional per-map power budget.**

- **(Recommended) 190 kW**, i.e. 190 affordable units world-wide at a `PLACEHOLDER`
  1 kW a unit and a `PLACEHOLDER` 40% reserve, **at 20 craters/s** — and record the
  destruction rate *in the same decision*, because it moves the answer from 6 873
  units to 55 across the measured range.
  *Downside:* two `PLACEHOLDER`s sit inside the number (the reserve and kW per
  unit), and the third input — G2's repath load — is charged rather than measured.
  It is provisional in a strong sense and must be re-derived at S2's exit.
- *Alternative — set the budget from the quiet-map figure (6 873 units).*
  *Downside:* the map would supply power for a unit count the tick cannot carry the
  moment destruction starts, which is the one configuration the game is always in.
- *Alternative — defer the number entirely to S2's exit.*
  *Downside:* the generator is built at the walking skeleton and needs a ceiling;
  "no number" means the first generator is written against an implicit one that
  nobody wrote down.
- *Alternative — set a lower reserve (say 25%) and publish a larger budget.*
  *Downside:* the plan's own reasoning is that a synthetic tick flatters the real
  one, and this tick is 96% substituted, so if anything the reserve is thin rather
  than generous.

**(3) Full or incremental per-tick state hash — a contract decision.**

- **(Recommended) Full.** Re-hash every table every tick.
  *Downside:* it gives up a measured 6.5% of the bytes encoded and 6–13% of the hash
  phase's time — about 0.0011–0.0025 ms a tick, which is 0.02% of the gate tick and
  under 1% of the part of it that is not the substitute.
- *Alternative — incremental at whole-table granularity* (what the spike built).
  *Downside:* it buys that saving with a dirty flag on **every write site in the
  sim**, forever, on the determinism artefact itself. A missed flag produces a
  *wrong* hash, not a slow one — the failure mode is a desync found days later on
  another operating system. And at 20 craters/s it has almost nothing to skip
  (7.641 of 8 tables re-encoded), because `Units`, `Header`, `Beacons`, `Seats` and
  `Voxels` are all dirty on essentially every tick.
- *Alternative — incremental at a finer granularity (per row, per chunk).*
  *Downside:* strictly more state to maintain and more flags to miss, for a saving
  the spike has no evidence is larger; the one fine-grained scheme that *is*
  already in place — per-chunk voxel digests — is keeping 0.834 ms out of the tick
  and is not up for discussion.
- **Whatever is chosen, do not hash the chunk store's bytes.** 0.834 ms against
  0.019 ms for the eight-table hash, and three times the whole non-pathing tick.

**(4) The SoA table layout and the broadphase structure the skeleton starts from.**

- **(Recommended) Keep both, with one caveat written down.** Vec-backed
  structure-of-arrays tables with the counts fixed at construction (the spike ran
  300 and 1 200 units from the same code with **zero allocations per tick**), and a
  CSR uniform grid rebuilt every tick by counting sort, cell edge = the largest
  query radius so a radius query touches at most 3 × 3 cells. Rebuild is
  O(items + cells) and allocates nothing after construction. The caveat: **treat the
  cell size as a tuning value tied to density, and expect the query to need work
  before the unit ceiling doubles** (§9.6).
- *Downside:* the query is super-linear in unit count — +34% quadratic share at 600
  units, marginal cost per unit up 72% across the sweep — so the structure that is
  28% of the quiet tick today is 47% at twice the load.
- *Alternative — a two-level or hierarchical grid now.*
  *Downside:* it is a solution to a problem the gate load does not have (0.22 ms at
  300 units), and it adds a second structure to keep deterministic and hashed. The
  spike's evidence says "watch it", not "replace it".
- *Alternative — a sorted-by-cell array with binary search, or a BVH.*
  *Downside:* the BVH costs a rebuild that is not O(items + cells) and is far harder
  to make allocation-free; the sorted array is what the CSR already is. Neither
  addresses the actual mechanism, which is that a 3 × 3 neighbourhood holds more
  candidates as density rises.
- *Alternative — keep an `imbl` persistent map for some tables, as G4 does for
  kill credit.*
  *Downside:* the spike deliberately used a dense table instead and measured zero
  allocations a tick; a persistent map puts small-allocation traffic back into the
  hot loop, which is the exact thing plan §5's allocator row warns makes the budget
  platform-dependent.

**(5) Is the 3/5/8 segment ladder affordable as written?**

- **(Recommended) Yes — keep 3/5/8.** A full 8-minute segment shows **no drift**:
  the p50 moves 0‰ and the mean −7‰ from minute 1 to minute 8 at the gate
  configuration, and +16‰/+11‰ with destruction off, which is scheduling noise on a
  machine with turbo enabled. The copy-on-write chunk store and the accumulating
  wrecks cost nothing measurable over 9 600 ticks.
  *Downside:* the drift check is a check on *this* tick, which does not contain a
  mesher, a renderer, a gateway or a verifier. The structures most likely to leak
  over eight minutes — the chunk store's `Arc` sharing and the wreck population —
  are in it, but the thing that would make an 8-minute segment expensive in the
  real game (memory growth across the whole process) is not.
- *Alternative — shorten the ladder (the spec's §6 fallback).*
  *Downside:* it discards a measurement that came back clean, and it interacts with
  the fun gate, whose own fallback ladder *starts* with "shorter default Push ladder
  with more rounds". If both gates ever point that way the change should be made
  once, deliberately — not pre-emptively here.

**(6) The per-tick repath cap** — not one of §8's five, but the lever the
measurement actually needs, and decisions-log item 60 already defines it with its
value left to S3.

- **(Recommended) 15–16**, re-derived at S2's exit once the burst frequency is a
  measurement. 16 is sustainable (mean backlog 2.85, no growth over a full segment,
  100% served) and turns G3′-a from FAIL to PASS at no measurable cost on the mean;
  15 is the ceiling the p99 arithmetic gives.
- *Downside:* the window is narrow — the stability floor is 9.45 and the p99 ceiling
  is 15.4 — and both ends move with constants this spike did not measure.
- *Alternative — 32.* *Downside:* **fails G3′-a** at 28.1 ms p99.
- *Alternative — 8, or anything below ~9.5.* *Downside:* it is the cheapest row in
  the sweep and it is not a configuration: the backlog grew by 3 092 over the
  segment and 14% of the modelled work was dropped for ever. A cap below mean demand
  buys a cheap tick by not doing the work.
- *Alternative — no cap.* *Downside:* 56 ms p99, i.e. a tick that misses its
  deadline outright whenever a burst lands.

### 9.17 Lessons for the skeleton

What surprised the measurement, and what the real crates must do differently.

- **The sim was never the risk; the pathfinder's load was.** Outside the substitute
  the whole eleven-phase tick costs **0.27 ms** at the gate configuration — 1% of
  the 25 ms budget. Everything the gate is close to is G2's. S2 should budget its
  attention accordingly: the interesting question is not "is the tick fast enough"
  but "how much repathing may a tick be asked to do", which is a *cap* question, not
  a *speed* question.
- **The mean and the p99 fail for different reasons and have different fixes.** The
  mean is a unit-count problem (fix it with the power budget, which is what the gate
  row says). The p99 is a burst problem (fix it with the per-tick cap). Conflating
  them produces the wrong lever — and note that §6's two named fallbacks, lower
  power budget and shorter segments, only address the first.
- **The broadphase is the sim's tick, and it is the first thing to outgrow the
  load.** 86–89% of everything that is not the substitute, super-linear in unit
  count, +72% marginal cost per unit from 50 to 600 units. Keep the CSR grid; plan
  for the cell size to become a density-tied tuning value.
- **An incremental hash has almost nothing to skip in a live tick.** The obvious
  optimisation measured 7.641 of 8 tables re-encoded, because in a running match
  almost every table is dirty almost every tick. This is worth knowing *before*
  someone spends a stage on it, and it is why the recommendation is the boring one.
- **Charge substituted costs explicitly, and label them in the artefact.** The one
  design decision that made this spike readable was splitting the `pathing` phase
  into `charged_ms` and `real_ms` and marking it `"substituted": true` in the JSON.
  Without it every number in §9.3 would read as a measurement of the sim. Any
  harness that stands in for a component it does not have should copy this.
- **Put a `sustainable` flag next to every throughput number.** The cheapest row in
  the repath-cap sweep is the one where the queue never drains. A sweep table
  without backlog growth in it actively misleads, and the fix is one boolean.
- **A number that cannot vary across platforms must not be compared across
  platforms.** The gate tick total reproduces itself on any machine by construction,
  so a green three-OS matrix on it would be a false reassurance. Emit the part that
  *can* vary (`tick_minus_pathing_mean_ms`) and compare that. The general lesson for
  harness part 1: know which column of a CI matrix carries signal.
- **Keep the calibration constant outside hashed state, and test that it is.** It is
  what lets three operating systems that run at three speeds be compared on one hash
  stream with `cmp`. The same applies to the repath cap and the burst rate: anything
  that is a performance knob must not touch the determinism artefact, and
  `tests/spike.rs` asserts it rather than asserting it in a comment.
- **Zero allocations per tick is achievable and worth defending.** It took removing
  exactly one `Vec` return from the kill-credit settlement path. With it, plan §5's
  whole allocator risk row evaporates and the budget stops being
  allocator-dependent. The real sim should have an allocations-per-tick assertion in
  its perf gate from S1, not as a later hardening task.
- **Clocks are worth recording even on a desktop.** The per-core performance counter
  showed the pinned core sitting at 108–112% of nominal through every run, which is
  what lets §9.10 say "no leak" instead of "no leak, probably". `CurrentClockSpeed`
  on Windows is useless for this; the `\Processor Information(N,M)\% Processor
  Performance` counter is not.
- **The lint wall worked, and is worth copying** — the same conclusion G2 reached.
  The library is sim code under the product's determinism set; `Instant` lives only
  in `src/bin/*.rs`; a test checks the confinement against the source text. The real
  crates get this from the workspace lint table, but they should copy the *test*.

### 9.18 The tuning values these numbers depend on

Every one is a spike-local constant that the skeleton must re-fix as rules-table
data (AGENTS.md §12: *"tuning values are data"*, stamped into the rules hash), and
every number above moves if one of them moves:

- **Load:** 4 seats, 300 units, 40 beacons, 2 structures per beacon (80), map
  384 × 384 × 64 voxels, match seed `0x123456789ABCDEF0`.
- **Time:** 20 Hz tick, decision period 5 ticks (250 ms), segment 9 600 ticks,
  minute bucket 1 200 ticks.
- **Broadphase:** query radius 24 voxels = cell edge, 16 × 16 = 256 cells.
- **Units and combat:** HP 150 / 2 400 / 12 000 (unit / structure / beacon), speed
  32 768 raw (0.5 voxels a tick), turn rate 512 angle units a tick, attack range 6
  voxels, arrive radius 2, damage 7, spread 3, cooldown 12 ticks, projectile TTL 10,
  route length 8 waypoints.
- **Economy and power:** bounties 250 / 600 / 1 200, beacon income 40, supply 95 kW
  a beacon, draw 40 kW a beacon and 25 kW a structure, **`UNIT_KW = 1`
  (`PLACEHOLDER` — this is the kW-per-unit the power budget multiplies by)**,
  upkeep 1 and 2, rebuild sweep every 200 ticks, 8 per sweep at 200 `$`.
- **Destruction:** 20 craters/s at the gate (the single most important number in
  §9.7 — the budget is 6 873 units at 0/s and 55 at 40/s).
- **Charged from G2, not measured here:** 9.45 repaths per crater at 300 units,
  0.77 ms a repath with 1.84 ms once in a hundred, 2.5 ms of cluster repair per
  crater, 0.5 ms of CSR rebuild per tick, burst factor 8×.
- **`PLACEHOLDER`s, which are decisions rather than constants:** the repath **burst
  frequency** (1-in-64 here; owner with G2 at S2's exit), the **reserve** in the
  power-budget arithmetic (40% here; owner at S2's exit), **kW per unit** (1 here;
  Tuning, spec section 19), and the two CI regression alarms in `ci/spike-g3.yml`
  (190 ms p99 / 40 ms mean; owner at S2's exit).

### 9.19 Verdict, and what still has to happen

- **G3′-b (the binding constraint): GO.** 10.620 ms mean, 4.708× real time, 15%
  inside the bound — with a synthetic tick that is 96.5% G2's charged cost, so read
  it as *"G2's pathing load fits at this segment length"*, not as *"the sim is
  fast"*.
- **G3′-a: conditional.** FAIL at an unbounded repath cap (56.23 ms p99), PASS at a
  cap of 16 (16.47 ms), FAIL again at 32 (28.06 ms) — and the failure at an
  unbounded cap is a readout of the unmeasured burst frequency (§9.2), not of the
  sim. **Neither lever the spec names as the fallback is needed. The lever that is
  needed is the per-tick repath cap, which decisions-log item 60 already defines:
  recommend 15–16, with a stability floor of 9.45 below which the queue never
  drains.**
- **G3′-c: met**, at world totals. At per-seat counts the configuration does not fit
  and no tuning value recovers it (§9.9) — the owner must settle which the gate row
  means.
- **The 3/5/8 segment ladder is affordable as written**: no drift over a full
  8-minute segment, on the mean or the p50, at either destruction rate.
- **Provisional per-map power budget: 190 kW** — 190 affordable units world-wide at
  a `PLACEHOLDER` 1 kW a unit and a `PLACEHOLDER` 40% reserve, at 20 craters/s. It
  is 446 units at 10 craters/s and 55 at 40, so **this number is a statement about
  the destruction rate as much as about the tick**, and the two must be set together
  at S2's exit.
- **Recommendations to the owner:** take the full hash (§9.5 — a contract decision,
  and the reason is structural rather than a 0.0025 ms saving); set the repath cap at
  15–16; keep the SoA tables and the CSR grid but treat the **broadphase** as the
  thing to watch (§9.6: super-linear, +34% quadratic share at 600 units, and 86% of
  the sim's own tick outside the substitute); and **give the repath burst frequency
  a measured value before anyone quotes a p99 from this spike**.

Still outstanding:

- **The §8 decision needs its `docs/design/decisions-log.md` §2.7 entry.** That file
  is a contract file (AGENTS.md §5): the entry is an owner decision, raised in the
  PR, not written by an agent. §9.16 lays the six items out as candidates with
  recommendations so they can be taken one at a time; item (3) is part of the
  determinism contract and needs owner approval alongside G4's.
- **The cross-OS half of plan §3 step 7 and of the hash-identity check** is pending
  the first run of `ci/spike-g3.yml`, which is not yet installed in
  `.github/workflows/` (§9.14).
- **The `PLACEHOLDER` CI thresholds in `ci/spike-g3.yml`** (190 ms p99 / 40 ms mean,
  loose regression alarms rather than gates) are replaced at S2 by the budget the
  sim's own perf gate sets.
- **S2 certifies this on real code.** Section 16 puts the synthetic check here and
  the measurement at S2's exit; what this spike fixes is the shape of the budget and
  the lever that controls it, not the number.
