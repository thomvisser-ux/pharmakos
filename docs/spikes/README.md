<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# Stack spikes — the 3-week stage before the walking skeleton

Spec: section 16 (Stack spikes & gates), section 15 (Architecture), section 17 (Build plan).
This stage is row 1 of the build plan: **ends wk 3, "Stack spikes", 3 weeks — G1 mesher and remesh · G2 pathing (HPA*) · G3′ synthetic tick check · G4 determinism with save/restore, plus fork equivalence in the research build. G5 deferred, G6 cancelled.**

## What a spike is here

The spec states the rule in section 16:

> Stack spikes replace a longer up-front hardening phase with 3 weeks of throwaway spikes. They cover only the gates whose failure would change the foundation itself. Every other gate runs on real code at the point where that code first exists, and each one's fallback is written down before its stage starts, because a late failure costs rework instead of a cheap spike.

So a spike in this stage is:

- **A measurement, not a feature.** Each spike exists to turn one number from a guess into an observation, and to produce one written decision. It is finished when the number is recorded and the decision is written down — not when the toy is nice.
- **Scoped to a foundation risk.** Only four gates qualify: G4, G1, G2+P3, G3′. Their failure would change the foundation (the determinism contract, the renderer, the pathing design, the map's power budget and segment lengths). Every other gate in the section 16 table — P1, P6, P7, P2, and the fun gate — runs on real code at the stage where that code first exists, and is not spiked here.
- **Not a go/pivot decision.** Section 16: *"One gate decides the project… the fun gate at wk 35.5… is the only go/pivot point. Everything else in this table certifies performance: it can move a number or a stage, but it cannot stop the project."* A failed spike takes its written fallback and the project continues.
- **Timeboxed.** Each plan below carries a day estimate. A spike that overruns its estimate by more than 50% is stopped where it stands, and its fallback is taken on the evidence collected so far. Overrunning is itself a result: it says the risk is larger than the foundation assumed.

## The throwaway rule

All spike code lived in `spikes/` at the repo root, **kept out of the cargo workspace** and out of `cargo xtask ci`; the directory is gone from `main` and survives at the `spike-end` tag (rule 3 below). The rule has three parts:

1. **No spike file is merged into the product.** Nothing under `spikes/` is moved, copied file-for-file, or `include!`d into `crates/`. When the skeleton needs the same algorithm, it is written again against the real types, with the spike open on the other monitor. This is deliberate: spike code has no lints, no error handling, no licence discipline and no tests worth keeping, and importing it would smuggle all of that into the contract layer.
2. **What survives is written, not compiled.** Each spike produces (a) the measured numbers, recorded in the *Results* section of its own plan file in this directory; (b) one decision, recorded in `docs/design/decisions-log.md` under §2.7; (c) at most a handful of short code excerpts pasted into the plan file where the exact shape of an API or a data layout is the finding.
3. **The directory is frozen, then deleted — done.** The `spike-end` tag was cut on **2026-09-14**, once all four spikes were closed and harness part 1 was green, and `spikes/` and the four `.github/workflows/spike-*.yml` were deleted from `main` in the same harness PR that added `crates/mesher` (decisions-log §2.7 item 71). The tag keeps the code readable and checkable forever: `git worktree add ../pharmakos-spikes spike-end`. An agent re-writing the mesher, HPA\* or the tick against the real types works with that worktree open — and rule 1 holds either way, so spike code is still never copied into `crates/`. Nothing in the shipped build, the licence manifest or the release zips references it.

Two consequences worth stating out loud, because they are easy to get wrong:

- **Spikes do not change contract files.** Section 15's dev-harness row: *"Contract files (.proto, lints, determinism) need owner approval."* A spike may *propose* the lint set, the snapshot format or the hash input encoding; those land in harness part 1 (wks 3–5) as reviewed contract changes, after the spike stage ends.
- **Parallelism is capped.** Section 15: *"at most 2–3 agents in parallel worktrees."* The schedule below assumes two worktrees running at once and the owner as the review bottleneck.

## Schedule inside the 3-week budget

One developer directing agents; 15 working days; two parallel worktrees.

| Week | Worktree A | Worktree B | Owner |
|---|---|---|---|
| wk 1 | **G4 determinism** (4 d) | **G1 remesh** starts (Godot + gdext + chunk store) | Install the toolchain (day 0), review G4's hash-input decision |
| wk 2 | **G2+P3 pathing** (4 d) | **G1 remesh** finishes (5 d total) | Review G1's ArrayMesh-vs-RenderingServer decision |
| wk 3 | **G3′ synthetic tick** (2 d) | Write-ups, results sections, decisions-log entries (2 d) | Decision memo; sign off the four decisions; cut the `spike-end` tag — **done 2026-09-14; `spikes/` and the spike workflows deleted with it** |

| Spike | Estimate | Owner-review points |
|---|---|---|
| G4 Determinism | 4 days | hash input encoding; snapshot format (rkyv vs postcard); RNG stream split |
| G1 Destruction remesh | 5 days | mesh upload path; per-frame upload budget; destruction granularity |
| G2 + P3 Pathing | 4 days | cluster size and level count; integer cost model; estimator API |
| G3′ Segment budget | 2 days | provisional per-map power budget; SoA layout; incremental hashing |
| Write-up and decision memo | 2 days (reserve) | — |

Total 17 agent-days over 15 calendar days with two worktrees. The 2-day reserve is the first thing spent when a spike overruns; when the reserve is gone, the timebox rule applies.

**Day 0 is not free.** No toolchain is installed on the build machine as of this writing: no `cargo`, no `protoc`/`buf`, no Godot. Every plan below starts with a *Prerequisites* step. Day 0 installs the Rust toolchain (MSVC ABI on Windows), Godot 4.7, and the CI runner images' equivalents; nothing in these plans can be compiled before that.

## The four spikes and their fallbacks

Gate text is copied verbatim from the section 16 table. The fallback is the spec's own, and it is written here *before* the stage starts, as section 16 requires.

| Gate | Go if (verbatim) | Fallback (verbatim) | Plan |
|---|---|---|---|
| **G4 Determinism** — Spike, wk 1–3 | "Identical hashes on 3 operating systems over 10 full matches (toy sim); save/restore round-trips hash-identically; fork equivalence in the research build" | "Replays guaranteed on the same binary only" | [G4-determinism.md](G4-determinism.md) |
| **G1 Destruction remesh** — Spike, wk 1–3 | "Own greedy mesher in Godot 4.7 remeshes within 3 frames at 60 fps with 20 explosions/s" | "Coarser destruction; smaller world; reopen the renderer choice" | [G1-destruction-remesh.md](G1-destruction-remesh.md) |
| **G2 + P3 Pathing and travel estimates** — HPA\* prototype in the spike, wk 1–3; certified in the sim at S3 | "Repath p99 ≤5 ms; travel estimate ≤1 ms and within ±15% (p90) on static terrain" | "Show ranges; favour arrival triggers" | [G2-P3-pathing.md](G2-P3-pathing.md) |
| **G3′ Segment budget** — Synthetic tick check in the spike; the real measurement is S2's exit; scrub at S7 | "4 seats (headroom over v1's 3), 300 units, 40 beacons: tick p99 ≤50% of the tick; ≥4× speed on the headless sim runner. The measurement sets the per-map power budget for the generator. Scrub ≤1 s is judged at S7" | "Lower map power budget; shorter segments" | [G3prime-segment-budget.md](G3prime-segment-budget.md) |

### What each fallback actually costs

The spec gives the fallback as a phrase. Each plan expands it into the concrete edits that would follow; in summary:

- **G4 fails → "Replays guaranteed on the same binary only".** The deterministic replay (seed + playbooks + log) stays a same-binary artefact: it still serves scrub, recap and bug reports on the machine that made it, but a cross-OS hash job in CI becomes advisory rather than blocking, bug reports must carry the build id, and the "cross-OS hash equality guards replays and CI" sentence in section 15 is downgraded. Shared recordings (keyframes plus deltas) are unaffected — they carry no playbooks and replay no sim.
- **G1 fails → "Coarser destruction; smaller world; reopen the renderer choice".** In that order: coarsen destruction (edit 2³ voxel groups instead of single voxels, halving dirty-chunk churn), then shrink the map below 384×384×64, then — only if both are insufficient — reopen whether Godot's RenderingServer is the right renderer for this world. The last option is expensive and is the reason G1 is a wk-1 spike rather than a skeleton task.
- **G2+P3 fails → "Show ranges; favour arrival triggers".** The editor stops promising an ETA it cannot meet: routes show a reachable-range band instead of a travel time, and playbook conditions lean on arrival triggers ("when the group arrives") rather than clock predictions. This changes the editor and the playbook vocabulary, which is why it must be known before the skeleton fixes them.
- **G3′ fails → "Lower map power budget; shorter segments".** The generator's per-map power budget drops until the unit count fits the tick, and if that is not enough the segment-length ladder (3/5/8) shortens. Both are numbers, not rewrites — which is why G3′ is only a synthetic check here and its real measurement is S2's exit.

## What each spike must produce

Every plan file in this directory has the same shape, and a spike is not done until every heading is filled in. A plan may insert a section of its own where its measurement needs one — G1 does, with a "How to measure frame time headlessly" section, which is why its later headings are numbered one higher than the list below — as long as **Goal is first and Results is last**:

1. **Goal** and the **exact go/no-go criterion**, copied from the section 16 table.
2. **The toy build** — which crate or Godot scene, under `spikes/<name>/`, out of the workspace.
3. **Measurement procedure**, step by step, with the exact numbers to record.
4. **CI hook** — what runs in GitHub Actions, on which runners, and what it asserts.
5. **Platform risks** — Windows/MSVC vs Linux (and macOS where it is in scope).
6. **Fallback if it fails** — the spec's phrase, expanded into the edits it implies.
7. **Estimate in days** within the 3-week budget.
8. **The decision the spike must produce** — one sentence that goes into `docs/design/decisions-log.md`.
9. **Results** — filled in at the end of the spike; empty until then.

## Conventions

- Numbers marked `PLACEHOLDER` in these plans are values that cannot be known before the toolchain exists or before a machine has been measured. Replace them with a measurement, never with a guess.
- The machine used for every timing number is recorded once, in each plan's Results section: CPU model, core count, RAM, GPU, OS build, Rust version, Godot version. A timing number without its machine is not a result.
- Commits in this stage are DCO signed off (`git commit -s`) like every other commit in the repo.
