<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->

# G1 — Destruction remesh (spike, wk 1–3)

## 1. Goal and go/no-go criterion

**Goal.** Prove that our own greedy mesher, written in Rust and feeding Godot 4.7 through a thin gdext crate, can keep up with the destruction the game is made of. Section 2 of the spec calls craters and breaches "what a Push is made of"; section 15 fixes the renderer decision as *"Our own greedy mesher in Rust feeds Godot RenderingServer RIDs under a per-frame upload budget. Voxel Tools is optional."* G1 is the measurement that either confirms that row or reopens it — which is why it is a wk-1 throwaway and not a skeleton task.

**Go/no-go criterion, copied verbatim from the section 16 gate table:**

> **G1 Destruction remesh** — Spike, wk 1–3 — Go if: *"Own greedy mesher in Godot 4.7 remeshes within 3 frames at 60 fps with 20 explosions/s"* — Fallback: *"Coarser destruction; smaller world; reopen the renderer choice"*

Read precisely, that is:

| # | Assertion | Pass condition |
|---|---|---|
| G1-a | Latency | Every chunk dirtied by an explosion is re-meshed **and visible** within 3 frames of the tick that edited it. At 60 fps, 3 frames = 50 ms — which is also exactly one 20 Hz sim tick, a pleasant coincidence worth holding on to. |
| G1-b | Load | The above holds under a sustained **20 explosions per second** — one explosion every 3 frames at 60 fps — not a single burst. |
| G1-c | Frame rate | The frame itself stays at 60 fps while this happens: the remesh must not be achieved by blowing the frame budget. Recorded as frame time p50/p99/max and the count of frames over 16.67 ms. |

The gate says nothing about a specific world size, so the spike uses the spec's map: **about 384×384 voxels wide by 64 tall** (section 9), in **32³ copy-on-write chunks** with a **64-layer cap** (section 15) — that is 12 × 12 × 2 = **288 chunks**, of which a fraction are non-empty.

## 2. The toy build

`spikes/g1-remesh/` — a Rust cdylib gdext crate plus a Godot 4.7 project, plus a headless Rust bench binary. Standalone workspace; thrown away.

```
spikes/g1-remesh/
  Cargo.toml            # [workspace] (empty) + cdylib; godot = "=0.5.5" (gdext, api-4-7)
  src/
    lib.rs              # gdext entry point, the GDExtension class exposed to the scene
    chunk.rs            # 32^3 chunk, copy-on-write, palette of material ids (u8 per voxel)
    world.rs            # 12x12x2 chunk grid, 384x384x64 voxels, dirty-chunk set (ordered)
    greedy.rs           # greedy mesher: 6 face directions, merged quads, vertex colours
    explode.rs          # sphere-of-voxels edit, deterministic, returns the dirty chunk list
    upload_arraymesh.rs # path A: ArrayMesh + MeshInstance3D
    upload_rs.rs        # path B: RenderingServer.mesh_create + instance_create2, direct
    budget.rs           # per-frame upload budget: queue, drain policy, counters
  src/bin/
    meshbench.rs        # headless, no Godot: mesher CPU cost per chunk (this is the honest CPU number)
  godot/
    project.godot       # Godot 4.7; vsync off; delta smoothing off; Forward+ ; window 1920x1080
    main.tscn           # camera at a fixed vista pose, the world node, a CSV logger
    main.gd             # drives the run: N frames, 20 explosions/s, writes frame-time CSV, quits
  runall.ps1            # the whole measurement set of section 3, in one deterministic order
  analyse.py            # CSV -> the rows of section 3's results table
  ci/
    spike-g1.yml        # section 5's workflow
    compare_vista.py    # the geometry job's assertion: golden-image compare, fails closed
    golden/vista_ci.png # the golden itself (committed; see section 10.7)
```

**What the mesher does.** Greedy meshing on a 32³ chunk: for each of the 6 face directions, sweep the 32 slices, build a 32×32 mask of visible faces keyed by `(material, light)`, and merge each maximal rectangle into one quad. One surface per chunk with a palette material; colour and the flood-fill voxel light go into vertex colours, as section 15's art-pipeline row describes ("flood-fill voxel light baked into vertex colours on remesh"). Neighbour-aware at chunk borders: a chunk needs its 6 neighbours' boundary slices to avoid meshing faces that are actually interior, which is why one explosion straddling a border dirties more chunks than it visually touches.

**Both upload paths are built**, because choosing between them is one of the spike's outputs:

- **Path A — ArrayMesh.** Build a `PackedVector3Array`/`PackedColorArray` set, `ArrayMesh.add_surface_from_arrays`, assign to a `MeshInstance3D`. Idiomatic, debuggable in the Godot inspector, costs a trip through Godot's Variant/Packed array marshalling and allocates a new mesh resource per remesh.
- **Path B — direct RenderingServer.** `RenderingServer.mesh_create()` once per chunk to get a **RID**, `mesh_add_surface_from_arrays` on first build, then `mesh_surface_update_vertex_region` / `mesh_surface_update_attribute_region` for in-place updates when the vertex count is unchanged, with `instance_create2` + `instance_set_transform` + `instance_set_scenario` for placement. No scene-tree node per chunk, no resource churn — but the surface format must match exactly on update, the RIDs must be freed by hand, and the calls must be made on the rendering thread (`RenderingServer.call_on_render_thread` when the render thread is separate).

**The per-frame upload budget** is the knob the spec already assumes exists. Implement it as a bounded queue: the sim marks chunks dirty; each frame the drain loop uploads at most *K* chunk surfaces and at most *B* bytes, whichever binds first, taking chunks in a deterministic order (nearest-to-camera first, ties by chunk index). *K* and *B* are what the spike measures. A chunk re-dirtied while still queued is coalesced, not queued twice.

**Determinism note.** Mesh output is presentation, not hashed state: the mesher's vertex data is floats and lives on the far side of the wall section 15 draws around presentation. The *input* — which voxels an explosion removes — is integer and deterministic and belongs to the sim. The spike must keep that line visible, because the float-arithmetic lint has to carve out exactly this one stage and no more. Getting the carve-out wrong is how floats leak into the sim later.

## 3. Measurement procedure

**Step 0 — Prerequisites.** Install Godot 4.7 (**4.7.2-stable**, `ed1daf0bf`, as installed at day 0) and the Rust toolchain with the **MSVC** ABI on Windows; gdext cdylibs must match the ABI Godot was built with. Record both versions and the GPU.

**Step 1 — Mesher CPU cost, headless, no Godot.** `cargo run --release --bin meshbench`. This is the honest CPU number and it must be taken outside Godot, because Godot's `--headless` mode uses a dummy rendering driver and will not tell the truth about GPU or upload cost. Generate a representative world (terrain with a heightfield, some ore seams, a few structures), then mesh every non-empty chunk. Record, over ≥1,000 chunk meshings:

- mesh time per chunk: p50 / p90 / p99 / max, in µs;
- vertices, indices and **bytes per chunk surface**: p50 / max;
- the same three numbers for a *freshly cratered* chunk (craters raise face count sharply — a smooth hillside meshes into a handful of quads, a crater wall into hundreds);
- allocation count per meshing, and peak RSS for the whole world.

**Step 2 — Dirty-chunk fan-out.** For explosion radii of 2, 4 and 8 voxels, placed at 1,000 deterministic positions including chunk corners and edges, record the distribution of **dirty chunks per explosion** (p50 / p99 / max). A radius-4 sphere centred on a chunk corner touches up to 8 chunks; with border-neighbour rules the count can be higher. This number × 20/s is the sustained remesh rate the renderer must absorb, and it is the single biggest driver of the result.

**Step 3 — In-engine run, path A.** Run the Godot project with path A selected, camera at a fixed vista pose overlooking the explosion field, vsync **off** so frame time is measured rather than clamped to the refresh interval. `main.gd` drives a fixed script: 3,600 frames (1 minute at 60 fps), one explosion every 3 frames at a deterministic position, each explosion tagged with the frame it was issued on. Each frame, log a CSV row:

`frame, frame_time_ms, process_ms, physics_ms, draw_calls, primitives, video_mem_mb, queued_chunks, uploaded_chunks, uploaded_bytes, remesh_ms_this_frame`

Frame time comes from Godot's `Performance` monitors (`TIME_PROCESS`, and the render monitors for draw calls / primitives / video memory) plus a per-frame delta measured in the extension. Each explosion's chunks carry their issue frame, so when a chunk's surface is uploaded the extension records `latency_frames = upload_frame - issue_frame`; that series is the direct measurement of G1-a.

**Step 4 — In-engine run, path B.** Identical script, path B selected. Same CSV.

**Step 5 — Find the budget.** Sweep *K* (chunk uploads per frame) over {1, 2, 4, 8, unbounded} on the winning path. For each: latency p50/p99/max in frames, frame-time p99, frames over 16.67 ms, and the maximum queue depth reached. The chosen budget is the largest *K* that keeps frame-time p99 under 16.67 ms, and the pass condition is that this *K* also keeps latency p99 ≤ 3 frames. If no *K* satisfies both, G1 fails and the fallback ladder starts.

**Step 6 — Stress beyond the gate.** Repeat at 40 explosions/s and at radius 8, not to pass or fail, but to record how much headroom exists and where the cliff is. S2 (destruction and combat) will need this.

**Step 7 — Headless CI measurement.** See section 4: the honest *CPU* number is reproducible headlessly; the *frame* number is not, and the spike records both with the difference stated plainly.

**Step 8 — Both OSes.** Repeat steps 3–5 on Windows and on Linux on real GPUs (the two first-class platforms). Record both; the gate is judged on the worse of the two.

**Step 9 — A screenshot.** One image of a cratered vista from the fixed camera pose, on each OS, saved next to the results. It is the cheapest possible check that the mesher is producing correct geometry and not merely fast garbage — missing faces, T-junction cracks at chunk borders and inverted winding all show up instantly and none of them show up in a timing number.

### Numbers to record (the results table)

| Number | Unit | Gate part |
|---|---|---|
| Mesh time per chunk, p50/p99/max (flat / cratered) | µs | context for G1-a |
| Bytes per chunk surface, p50/max | KiB | budget *B* |
| Dirty chunks per explosion, p50/p99/max, by radius | count | load |
| Remesh latency, p50/p99/max | **frames** | **G1-a: p99 ≤ 3** |
| Sustained explosion rate held | /s | **G1-b: 20** |
| Frame time p50/p99/max | ms | **G1-c: p99 ≤ 16.67** |
| Frames over 16.67 ms, out of 3,600 | count | G1-c |
| Chosen *K* (chunks/frame) and *B* (bytes/frame) | — | the budget decision |
| Max queue depth | chunks | does the queue ever run away |
| Draw calls, primitives, video memory | — | path A vs B |
| Path A vs path B on every row above | — | the upload decision |
| Same, on Windows and on Linux | — | judged on the worse |

## 4. How to measure frame time headlessly

This deserves its own section because it is the part most likely to produce a confident wrong number.

**Godot's `--headless` does not render.** It runs with a dummy rendering driver: no GPU work is submitted, no buffers are uploaded, and the frame-time monitors report a world in which drawing is free. A headless "60 fps" means nothing for G1-c. So the spike splits the measurement in two:

1. **The CPU half is measured headlessly and is CI-gradeable.** `meshbench` (step 1) is a plain Rust binary with no Godot at all: mesher cost per chunk, fan-out per explosion, bytes per surface, allocations. Add a second headless binary that simulates the *budgeted pipeline* — issue 20 explosions/s of virtual frames at 16.67 ms, run the real dirty-set and the real drain policy, and compute the latency-in-frames series with upload cost modelled as `bytes / upload_rate`, the rate measured once in step 3/4 (**≈1.70 × 10⁶ B/ms** on this machine: the proxy charges the FFI byte count, 96 623 B/chunk, against path B's measured 56.9 µs/chunk). This gives a CI-stable proxy for G1-a that catches a regression in the mesher or the queue policy, and it is honest about being a proxy.
2. **The GPU half needs a real device and is recorded, not gated in CI.** The go/no-go frame-time number is taken on the developer machine's real GPU, on Windows and on Linux, from step 3/4's CSV.

**For an unattended in-engine run**, drive Godot windowed (not headless) with:
- `--rendering-driver vulkan` (record the driver actually chosen; see risks for D3D12),
- vsync disabled in `project.godot` (`display/window/vsync/vsync_mode = 0`) so frame time is not quantised to the display's refresh,
- a fixed camera pose and a deterministic explosion script, so runs compare,
- `--quit-after 3600` (quit after N frames) so the run terminates on its own, and the CSV is flushed in `_notification(NOTIFICATION_WM_CLOSE_REQUEST)`,
- **not** `--fixed-fps` and **not** `--write-movie`: both force a fixed timestep, which is exactly the quantity being measured.

On a CI runner with no GPU, the in-engine run can still execute under a software rasteriser (Mesa llvmpipe on Linux, WARP on Windows) to prove *it does not crash and the geometry is correct* (compare a rendered screenshot against a golden image with a generous tolerance). Its timings are meaningless and are labelled as such in the CSV header. This is also the shape the skeleton's "headless Godot screenshots" harness (section 15, harness part 2) will take, so the spike is prototyping it deliberately.

## 5. CI hook

`.github/workflows/spike-g1.yml` in the spike worktree. Three jobs, only the first two blocking:

```yaml
name: spike-g1-remesh
on: [push, workflow_dispatch]
jobs:
  mesher-cpu:                       # blocking; real numbers, no GPU needed
    strategy: { matrix: { os: [ubuntu-latest, windows-latest] } }
    runs-on: ${{ matrix.os }}
    steps:
      - run: cargo run --release --bin meshbench -- --json out.json
      # assert p99 chunk mesh time <= 1050 us flat / 1500 us cratered and
      # bytes/surface p99 <= 280 KiB (set from the 2026-09-14 measurement — flat
      # 341-405 us, cratered 480-486 us, 175 KiB — at ~3x headroom because hosted
      # runners are slower and much noisier, then held as a regression budget)
      - run: cargo run --release --bin meshbench -- --pipeline --explosions-per-s 20 --frames 3600
      # asserts modelled latency p99 <= 3 frames with the chosen K and B
  geometry:                         # blocking; correctness, not speed
    runs-on: ubuntu-latest
    steps:
      # golden-image compare of the fixed vista under llvmpipe; catches cracks,
      # missing faces and winding errors. Timings from this job are ignored.
      - run: godot --headless --path spikes/g1-remesh/godot --quit-after 120 --screenshot out.png
  frame-time:                        # advisory; no GPU on hosted runners
    if: false                        # enable only on a self-hosted GPU runner
```

The gate itself is judged from the developer-machine runs of steps 3–5, recorded in section 10. CI's role here is to stop the *CPU* side from regressing once the number exists.

## 6. Platform risks: Windows/MSVC vs Linux

| Risk | Why it differs | How this spike handles it |
|---|---|---|
| **gdext ABI / MSVC toolchain** | A GDExtension cdylib must be built with a toolchain compatible with the Godot binary. On Windows that is the MSVC ABI; a `-gnu` host Rust produces a library that loads unpredictably or not at all. | Pin `stable-x86_64-pc-windows-msvc`; record the gdext crate version against the exact Godot 4.7 patch. Godot's extension API is versioned — a mismatch is a hard failure at load, which is at least loud. |
| **Rendering backend: Vulkan vs D3D12 vs Metal** | Godot 4 on Windows can run Vulkan or D3D12; upload paths, buffer-orphaning behaviour and driver stalls differ between them, and between GPU vendors more than between OSes. | Record the driver and GPU on every run. Take the primary number on Vulkan on both OSes for an apples-to-apples comparison, then record D3D12 on Windows separately. macOS/Metal is out of scope for this gate — macOS is a CI artefact only (section 15). |
| **Render-thread affinity** | Godot may run rendering on a separate thread. `RenderingServer` calls from the wrong thread are a crash or silent corruption, and the failure often appears only under load or only on one platform's timing. | Path B routes every RID call through `RenderingServer.call_on_render_thread`, or the project pins the safe thread model; record which, since it constrains the skeleton's mesher design. |
| **Surface-update format strictness** | `mesh_surface_update_vertex_region` requires the update to match the surface's declared format and size exactly. A change in vertex count means a full surface rebuild, not an update — and greedy meshing changes the vertex count on almost every edit. | Measure how often an in-place update is actually possible versus a rebuild. If it is rare, path B's main advantage over ArrayMesh shrinks to resource churn, which changes the decision. Record the ratio. |
| **Index width** | 16-bit indices cap a surface at 65,535 vertices; exceeding it silently forces 32-bit (more bytes) or breaks. A heavily cratered 32³ chunk can approach that. | Record max vertices per chunk from step 1 and state which index width the format uses. If chunks can exceed the cap, that is an argument for a smaller chunk or a split surface. |
| **Frame pacing: DWM and vsync** | On Windows, desktop compositing and the default timer resolution inflate and quantise frame times; on Linux the compositor varies by desktop. Measuring with vsync on measures the display, not the renderer. | vsync off, windowed, exclusive of compositor effects where possible; three repetitions, report the median of the p99s; record the refresh rate and compositor. |
| **Allocator behaviour** | The Windows default allocator is markedly slower than glibc's for the many short-lived vectors a mesher produces, so the same code can look different on the two OSes for reasons that have nothing to do with rendering. | Measure allocations per meshing in step 1; reuse scratch buffers in the mesher rather than allocating per call; if Windows is still the outlier, record whether an alternative allocator closes the gap (a decision for the skeleton, not a fix for the spike). |
| **Antivirus and file watching** | Real-time scanning on Windows adds unpredictable latency to file and module loads, and can distort a long run. | Exclude the spike directory during measurement and say so in the results; never as a silent step. |
| **Panics across FFI** | A Rust panic unwinding into Godot is undefined behaviour and manifests differently per platform. | Every gdext entry point catches panics at the boundary and reports; a panic under load is a finding to record, not a crash to shrug at. |
| **Floats, but deliberately** | Floats are forbidden by lint outside a walled presentation module — and mesh vertices are floats. | The spike keeps the wall explicit: voxel edits and the dirty set are integer; the float boundary starts at vertex generation. The exact shape of the lint carve-out is one of the decisions this spike must produce. |

## 7. Fallback if it fails

The spec's fallback is **"Coarser destruction; smaller world; reopen the renderer choice"**, and the order is the order:

1. **Coarser destruction.** Explosions edit 2³ voxel groups instead of single voxels. Roughly an 8× cut in edited voxels and a large cut in face count per crater, with a visible but acceptable loss in crater detail. The sim's voxel grid is unchanged — only the *edit granularity* coarsens — so nothing in pathing, spheres or placement moves. Cheapest fallback by far and the one to reach for first.
2. **Smaller world.** Shrink the map below 384×384×64 (section 9 already says the size is "tuned by G3′", so it is a number both G1 and G3′ can push on). Fewer chunks, fewer resident surfaces, less video memory, and shorter travel — which touches map design and the spawn-distance rule, so it is more expensive than option 1.
3. **Reopen the renderer choice.** Only if 1 and 2 together are insufficient. That means re-evaluating Godot's RenderingServer as the delivery path: Voxel Tools (already noted as optional in section 15) as a supported alternative, or a different renderer entirely. This is a foundation change costing weeks, and discovering the need for it in week 1 rather than week 20 is the entire reason G1 is a spike.

A fourth, smaller lever is available before any of these and should be tried inside the spike itself if the budget sweep is close: **raise the latency target's honesty** by treating the 3-frame requirement as p99 rather than max, and letting a rare 4th-frame chunk through. The gate says "within 3 frames"; the spike reports both p99 and max so the owner can judge whether a max of 4 on 0.1% of chunks is a fail in spirit.

## 8. Estimate

**5 days**, in worktree B across weeks 1–2 of the 3-week budget. The longest of the four spikes, because it is the only one that involves a second toolchain and a GUI.

| Day | Work |
|---|---|
| 0 (shared) | Godot 4.7 + gdext install; empty extension loads in a scene; "hello from Rust" on screen |
| 1 | Chunk store, world grid, explosion edit, dirty set; `meshbench` skeleton |
| 2 | Greedy mesher with border-neighbour handling; step 1 and step 2 numbers recorded |
| 3 | Path A (ArrayMesh) + path B (RenderingServer RIDs); the budgeted upload queue; CSV logging |
| 4 | In-engine runs, budget sweep, stress run, screenshots; Windows and Linux |
| 5 | Headless CPU proxy + CI workflow; results write-up |

Owner review is needed once, at the end of day 4, on the upload-path and budget decision — it shapes the skeleton's one real vista (section 17: *"It includes one real vista (mesher, destruction, the watch rig) for demos"*).

## 9. The decision this spike must produce

> **How voxel geometry reaches the screen** — specifically: (1) ArrayMesh or direct `RenderingServer` RIDs, with the measured reason; (2) the per-frame upload budget as two numbers, *K* chunk surfaces and *B* bytes, plus the drain order; (3) whether 32³ chunks and single-voxel destruction survive, or whether the fallback ladder's first rung is taken now; (4) whether the mesher runs on the main thread or a worker pool, and exactly where the presentation wall sits so the float lint can be written around it in harness part 1.

Recorded in `docs/design/decisions-log.md` §2.7. Item (4) touches the lint set, which is a contract file and needs owner approval.

## 10. Results

Measured 2026-09-14 on the owner's build machine. The raw CSV/JSON/PNG artefacts live in
`spikes/g1-remesh/results/` and are git-ignored on purpose (`spikes/README.md`):
everything below is the record that survives them. `spikes/g1-remesh/build.ps1` builds
both halves and `spikes/g1-remesh/runall.ps1` reproduces the whole in-engine set in one
command; `meshbench` is the headless half.

Every number in this section was produced by the run described in §10.1 — 24 Godot
launches (6 headline, 10 *K* sweep, 4 *B* sweep, 2 stress, 2 vista), plus a 25th at the CI
resolution, plus four `meshbench` passes. Where a figure varies between repetitions the
spread is given rather than a single value, because on this machine the spread is
frequently larger than the difference the number is being used to decide.

### 10.1 Provenance

- **Machine:** Intel Core i7-9800X, 8C/16T @ 3.79 GHz · 31.7 GiB RAM · Windows 10 Pro
  19045 (10.0.19045) · desktop 1920×1080 @ 60 Hz, DWM compositing on.
- **GPU:** NVIDIA Quadro P4000, driver 32.0.15.5639 (WDDM, dated 2024-09-09),
  Vulkan API 1.3.278 as reported by the adapter.
- **Godot** 4.7.2-stable (official, `ed1daf0bf`) · **gdext** `godot` crate `=0.5.5`,
  features `api-4-7` + `__codegen-full` · **Rust** 1.98.1 stable-x86_64-pc-windows-msvc
  (rustc `48a229cea`, LLVM 22.1.8) · **rendering driver** Vulkan (Forward+),
  `rendering/driver/threads/thread_model = 1` (Single-Safe: the main thread owns the
  `RenderingServer`), MSAA off.
- **World:** 384 × 64 × 384 voxels in 288 chunks of 32³, seed 1, 283 non-empty chunks,
  5 148 702 solid voxels, 241 chunks that produce a surface by the end of a 3 600-frame
  run (230 by the end of a 900-frame one).
- **Measurement settings:** vsync off; `application/run/delta_smoothing = false`;
  `Engine.physics_jitter_fix = 0`; no `--fixed-fps`, no `--write-movie`; windowed
  1920×1080; the spike directory was **not** excluded from Windows Defender, so the
  numbers include whatever real-time scanning costs on this machine.

**Frame time is `ext_dt_us`, a raw monotonic delta taken inside the extension, not
Godot's `_process(delta)`.** `analyse.py` derives every frame-time figure from the raw
delta regardless of what the engine reports, so a future engine default cannot move a gate
number quietly. With `delta_smoothing = false` and `physics_jitter_fix = 0` the two series
agree to a few microseconds and `analyse.py`'s `delta_smoothing_suspected` heuristic was
`False` on all 25 runs; with the engine defaults in place (jitter fix 0.5) the reported
delta quantises to whole physics ticks and is not a measurement. Both switches are off.

**The run is frame-driven, not wall-clock-driven.** The gate's own wording is "one
explosion every 3 frames at 60 fps", and that is what the schedule implements. With vsync
off the median frame on this machine costs 0.64–0.70 ms and the *mean* frame — which is
what sets throughput — costs about 1.5 ms, so 3 600 frames complete in ~5.5 s of wall time
(≈650 fps sustained) and the world absorbs **209–220 explosions per wall-clock second**.
That is conservative for G1-c (the same per-frame work with less wall time to do it in)
and far *harsher* than the gate for G1-b. The one thing it does not
exercise is driver behaviour specific to a 60 Hz cadence.

### 10.2 Step 1 — mesher CPU cost (headless, no Godot)

`meshbench`, 1 000 chunk meshings per pass, each chunk meshed 5 times with the
**minimum** kept, run three times end to end. A single `Instant` sample per meshing is not
reproducible on Windows, so the single-shot figure is reported separately as `us_raw_*`:
the gap between the two is scheduler preemption, and hiding it would be the opposite
mistake. The three runs are given as ranges.

| | flat | cratered |
|---|---|---|
| mesh time µs, best-of-5 — p50 / p90 / p99 / max | 273–275 / 315–321 / **341–405** / 376–1 157 | 322–329 / 421–432 / **480–486** / 504–554 |
| mesh time µs, single shot — p50 / p99 / max | 285–288 / 364–413 / 497–1 168 | 335–340 / 510–512 / 568–592 |
| vertices p50 / max | 536 / 1 348 | 1 232 / 6 660 |
| indices p50 / max | 804 / 2 022 | 1 848 / 9 990 |
| bytes/surface, FFI p50 / p99 / max | 17.8 / 39.8 / 44.8 KiB | 40.9 / 175.2 / 221.1 KiB |
| bytes/surface, GPU p50 / p99 / max | 9.9 / 22.3 / 25.0 KiB | 22.9 / 97.9 / 123.6 KiB |
| allocations per meshing | 0.000 | 0.001 |
| empty surfaces (excluded from the geometry rows) | 312/1 000 (31.2 %) | 104/1 000 (10.4 %) |

The geometry rows are byte-for-byte identical across all three runs — the mesher is
deterministic and only the timings move. Notes:

- The geometry rows are over **non-empty surfaces only**. A fully buried chunk produces no
  surface at all, and averaging that into a byte percentile that then sets *B* is how a
  budget gets set a third too low.
- Two byte figures, deliberately: **FFI** is 28 B/vertex + 4 B/index, what
  `mesh_add_surface_from_arrays` carries across (`PackedVector3Array` +
  `PackedColorArray` + `PackedInt32Array`); **GPU** is 16 B/vertex + 2 B/index, because
  Godot stores 16-bit indices below 65 536 vertices.
- **Max vertices 6 660 ⇒ 16-bit indices are sufficient** with ~10× headroom, so a 32³
  chunk never needs a split surface or a 32-bit index buffer. This is also why path B
  cannot patch the index region by hand: the surface's indices are 16-bit while the
  mesher hands Godot 32-bit ones.
- Peak heap high-water for the whole world + mesher + buffers: **18.8 MiB**.
- Of the ~330 µs mean per cratered chunk, **12 µs is the neighbour-aware padded copy**;
  the rest is the fixed 6 × 32 × 1024 mask sweep, which runs whatever the geometry looks
  like and is what sets the p50.
- **The `max` row is preemption, and it moves.** The flat pass produced a best-of-5 max of
  376, 1 157 and 378 µs on three consecutive runs of identical work — a 3× spread on the
  tail with the p50 moving by 2 µs. Any CI budget set from a `max` will flap; the p99 of
  the best-of-5 sample (≤ 405 µs flat, ≤ 486 µs cratered here) is the number worth
  holding as a regression budget, with generous headroom.
- Worldgen: 116 ms cold in `meshbench`, 87–100 ms inside the engine.

### 10.3 Step 2 — dirty-chunk fan-out

1 000 deterministic placements per radius, a quarter of them snapped to chunk corners and
a quarter to chunk edges. Identical on all three runs.

| radius | p50 | p90 | p99 | max | mean | chunk remeshes/s at 20 expl/s |
|---|---|---|---|---|---|---|
| 2 | 4 | 8 | 8 | 8 | 5 | 100 |
| 4 | 8 | 8 | 8 | 8 | 5 | 100 |
| 8 | 8 | 8 | 8 | 8 | 6 | 120 |

The 2 × 2 × 2 chunk neighbourhood is the cap because the world is only two chunks tall,
and `DIRTY_PAD` (5 voxels) is smaller than a chunk. In the headline runs the observed rate
was 5 746 remeshes over 1 200 explosions = **4.79 chunks per explosion**, i.e. 1.6 chunk
uploads per frame on average against a budget of 4.

### 10.4 Steps 3–4 — in-engine, path A vs path B

3 600 frames (the plan's one minute at 60 fps), 20 explosions/s, radius 4, *K* = 4,
*B* = 512 KiB, **three repetitions each**. Frame time from the raw delta, steady state
(frames > 30).

| | path A (ArrayMesh) | path B (RenderingServer RIDs) |
|---|---|---|
| frame time p50, 3 reps | 0.671 / 0.669 / 0.704 ms | 0.639 / 0.686 / 0.698 ms |
| frame time p99, 3 reps (**median of the p99s**) | 4.686 / 4.001 / 6.278 → **4.69 ms** | 5.520 / 6.289 / 5.653 → **5.65 ms** |
| frame time max, 3 reps | 10.694 / 10.802 / 11.155 ms | 10.078 / 10.641 / 11.018 ms |
| **frames over 16.67 ms, out of 3 600 (steady state)** | **0 / 0 / 0** | **0 / 0 / 0** |
| frames over 16.67 ms, all frames | 1 / 1 / 1 (frame 2: 19.7 / 21.1 / 20.3 ms) | 1 / 1 / 1 (frame 2: 18.5 / 21.6 / 17.8 ms) |
| remesh latency p50 / p90 / p99 / max (frames) | 0 / 1 / 1 / **2** | 0 / 1 / 1 / **2** |
| chunks left queued at end | 0 | 0 |
| **upload cost per chunk** | 84.0 / 84.1 / 84.5 µs | **56.8 / 57.4 / 56.9 µs** |
| remesh cost per chunk | 450.9 / 446.9 / 467.8 µs | 459.2 / 465.8 / 464.5 µs |
| in-engine mesh time per chunk, p50 / p99 / max | 426–433 / 1 155–1 501 / 2 140–2 223 µs | 426–429 / 1 457–1 566 / 2 128–2 312 µs |
| bytes actually written per chunk | 96 623 (all FFI) | **77 025** (46 % of updates write 16 B/vertex) |
| chunk uploads / explosions | 5 746 / 1 200 | 5 746 / 1 200 |
| **mesh resources created** | **5 798 `ArrayMesh` objects** | **0** (241 RIDs, reused) |
| draw calls / primitives | 199 / 222 870 | 199 / 222 870 |
| video memory | 82.39 MiB | 82.89 MiB |
| panics caught at the FFI boundary | 0 | 0 |

**Path B wins on upload cost by 1.48×** (56.8–57.4 vs 84.0–84.5 µs/chunk, three
repetitions each, non-overlapping ranges) and creates no garbage: path A allocated and
dropped 5 798 `ArrayMesh` resources over the minute. Both paths produce **byte-identical
geometry** (§10.7), an identical draw-call count, identical primitive count and video
memory within 0.5 MiB, and both pass every gate row with the same latency distribution.

**The frame-time comparison does *not* separate the two paths, and this run says so more
clearly than a single repetition would.** Path A's median p99 came out *lower* than path
B's (4.69 vs 5.65 ms) while within-path spread was 4.00–6.28 ms for A and 5.52–6.29 ms for
B — overlapping ranges, opposite ordering to what the per-chunk upload cost predicts.
1.6 chunk uploads per frame × ~27 µs saved is ~43 µs, which is 0.3 % of a 16.67 ms budget
and roughly 2 % of the frame-to-frame noise; the p99 of a frame series whose median
frame is 0.7 ms is dominated by whatever else the machine is doing. **The upload decision therefore rests on
per-chunk upload cost and resource churn — both measured cleanly and both favouring
path B — and not on frame time, which is a tie.**

Path B's in-place region update was possible on **46.2 %** of updates to an existing
surface (2 570 in place, 2 987 rebuilt, 241 first builds; 0 rebuilds forced by a format
change). Two facts about that number:

- **The guard has to compare the index array, not the vertex count.** Each quad emits
  `v0,v0+1,v0+2,v0,v0+2,v0+3` or the reversed pattern depending on its face sign, so two
  remeshes with the same vertex *and* index counts can still need different indices when
  quads move between the six face directions. In this run **29 of the updates that a
  count-only guard would have patched in place had a changed index array** (identically 29
  in all three repetitions; 6 in a 900-frame run) — 29 chunk surfaces that would have
  rendered with stale winding, i.e. back-face-culled holes, until their next rebuild. No
  timing number would ever have shown it, and the vista only would have if a hole happened
  to fall in shot.
- **Godot's `Color` → RGBA8 conversion is a truncating cast, and it is probed rather than
  assumed.** The first surface is read back with `mesh_get_surface` and its
  `attribute_data` matched against both a truncating and a round-half-up conversion; only
  the matching one is used for in-place writes, and in-place updates are disabled outright
  if neither matches. Probed result on 4.7.2, printed identically on all 13 path-B runs:
  `vertex_data=528 attribute_data=176 vertex_count=44 -> strides 12/4 colour conversion
  Some(Truncate)`. With the probed conversion the two paths' vistas are **pixel-identical:
  0 of 2 037 120 pixels differ** (§10.7).

### 10.5 Step 5 — the budget sweep

*K* sweep, 900 frames, *B* unbounded, path B:

| *K* | latency p50 / p99 / max | chunks never uploaded (and their max age) | max queue depth | frame p50 / p99 / max | frames > 16.67 ms |
|---|---|---|---|---|---|
| 1 | 5 / 330 / 368 | **65** (861 frames) | 69 | 1.01 / 1.62 / 3.38 ms | 0 |
| 2 | 1 / 7 / 19 | **12** (19 frames) | 17 | 1.44 / 4.61 / 5.84 ms | 0 |
| **4** | **0 / 1 / 1** | 0 | 8 | 0.56 / 2.79 / 7.69 ms | 0 |
| 8 | 0 / 0 / 0 | 0 | 8 | 0.54 / 4.22 / 4.52 ms | 0 |
| unbounded | 0 / 0 / 0 | 0 | 8 | 0.53 / 4.36 / 8.42 ms | 0 |

Path A's *K* sweep agrees on every latency and queue row (identical figures: the queue
policy is the same code) and differs only in frame time, where it is again within noise:
p99 3.11 / 4.59 / 2.81 / 5.53 / 4.62 ms and max 3.64 / 5.53 / 8.33 / 13.51 / 9.14 ms for
*K* = 1 / 2 / 4 / 8 / unbounded.

The "chunks never uploaded" column matters: latency is only sampled when a chunk is
actually uploaded, so at *K* = 1 the 65 worst-aged chunks — the ones that made the queue
run away — contributed no sample at all and the p99 of 330 frames *flatters* a
configuration that never caught up. *K* = 1 and *K* = 2 are disqualified by that column,
not by their frame time, which is the best in the table.

*B* sweep at *K* = 4, 900 frames, path B:

| *B* | latency p99 / max | chunks never uploaded | post-drain queue depth | frame p99 / max |
|---|---|---|---|---|
| 128 KiB | 2 / 2 | 2 | 6 | 5.18 / 7.68 ms |
| 256 KiB | 1 / 2 | 0 | 4 | 4.60 / 8.14 ms |
| **512 KiB** | **1 / 1** | 0 | 4 | 2.70 / 2.87 ms |
| 1 MiB | 1 / 1 | 0 | 4 | 2.67 / 3.18 ms |

**Three deviations from the plan's decision rule, stated rather than buried:**

1. Plan §3 step 5 says "the chosen budget is the largest *K* that keeps frame-time p99
   under 16.67 ms". Every *K* satisfies that, including unbounded, so the plan's rule
   selects unbounded — which is the same as having no budget at all. **We chose the
   *smallest* K that meets the latency condition (*K* = 4, latency p99 1 and max 1).**
   This is a deliberate amendment to the plan's rule and the owner should ratify it; the
   rationale is in point 2.
2. **The rationale for the amendment is structural, not measured.** *K* bounds the work
   one frame can be asked to absorb: at *K* = 8 or unbounded a single frame can take an
   entire 8-chunk explosion fan-out, ~8 × 450 µs of meshing plus 8 uploads, and nothing in
   the policy prevents it. On *this* machine that bound is never the binding constraint
   and the frame-time max **does not** order itself by *K* — path B's max was *lower* at
   *K* = 8 (4.52 ms) than at *K* = 4 (7.69 ms), while path A's was higher (13.51 vs
   8.33 ms). At 900 frames the max is one sample and it is noise. So: choose the smallest
   *K* that satisfies latency because it bounds the worst case by construction, and do not
   claim a measured frame-time reason for it, because there is not one here.
3. **B = 512 KiB is a headroom margin, not a measured optimum.** *B* stops binding at
   about 256 KiB: 256 KiB, 512 KiB and 1 MiB are indistinguishable on chunks uploaded and
   queue depth, and only 128 KiB degrades anything (2 chunks never uploaded, latency max
   2). 512 KiB was chosen as one doubling above the point where it stops mattering, so
   that a heavier future surface format does not silently start clipping. For scale, the
   headline run's path-B surfaces averaged 75.2 KiB, so *K* = 4 spends ~301 KiB of the
   512 KiB in a typical fully-drained frame.

### 10.6 Step 6 — stress beyond the gate

Path B, *K* = 4, *B* = 512 KiB, 900 frames:

| | 20 expl/s, r=4 (the gate) | **40 expl/s**, r=4 | 20 expl/s, **r=8** |
|---|---|---|---|
| latency p50 / p99 / max | 0 / 1 / 1 | 0 / **3** / **7** | 0 / 1 / 1 |
| max queue depth | 8 | **18** | 8 |
| coalesced re-dirties | 0 | 23 | 0 |
| frame p50 / p99 / max | 0.55 / 2.70 / 2.87 ms | 2.37 / 3.00 / 3.20 ms | 2.18 / 3.74 / 10.57 ms |
| frames over 16.67 ms | 0 | 0 | 0 |
| bytes written per chunk | 33.6 KiB | 48.9 KiB | 61.5 KiB |
| voxels removed | 55 605 | 108 049 | 373 334 |

Doubling the explosion rate to 40/s is where the latency budget starts to be spent: p99
reaches 3 frames — the gate's limit exactly — and max reaches 7. That is the cliff, and it
is a *latency* cliff, not a frame-time one: the frame is still nowhere near 16.67 ms.
Raising *K* is the lever, and there is frame-time room to raise it. Radius 8 costs bytes
and mesh time (389 µs p50 in-engine against 360) but not latency, because the fan-out is
already capped at 8 chunks. Note that the 900-frame runs write far fewer bytes per chunk
than the 3 600-frame headline (33.6 vs 75.2 KiB): craters accumulate and surfaces grow
throughout a run, which is why a sweep length must never be used for a headline number.

### 10.7 Step 9 — the screenshot, and the CI geometry assertion

One 1920×1080 vista from the fixed camera pose after 880 frames of destruction, taken on
each path (the request is clamped to a 1920×1061 client area by the 1920×1080 desktop).
Inspected at 1:1: craters show correct interior geometry with the exposed material strata
(soil, stone, ore) and a darker interior, bunkers show their outside faces with correct
winding, and no crack, T-junction seam or missing face is visible anywhere in the frame.
The two paths' images are **byte-identical** (same MD5; `compare_vista.py` reports 0 of
2 037 120 pixels differing, worst channel delta 0).

**New this run: the CI `geometry` job's assertion has now actually been executed**, on
Windows/Vulkan rather than on the Linux/lavapipe runner it is written for. A 25th run at
the golden's resolution —

```
godot --path godot --rendering-driver vulkan --resolution 1280x720 -- \
    --path=b --frames=900 --eps=20 --radius=4 --k=4 --b=524288 --shot=880
python ci/compare_vista.py results/ci720/vista_ci.png ci/golden/vista_ci.png 6.0 0.02
```

— produced `1280x720, 3 channels`, red-channel variance 641.7, and **0 of 921 600 pixels
differing from the committed golden, worst channel delta 0: `geometry ok`**. So the
comparator parses Godot's PNG, the structural checks pass, and the golden is a true match
for what this machine renders. What is still unproven is the *portability* of that golden:
a different rasteriser (lavapipe) will not reproduce a bit-exact image, and whether the
6.0-mean / 2 %-hard tolerances are the right generosity for that case has not been tested
on any Linux machine.

`compare_vista.py` needs no Pillow and no numpy — it is stdlib plus `zlib` — which is just
as well, since Pillow is not installed on this machine.

**Two claims that were in the code and are not true, corrected here because they are what
gets copied into the skeleton:**

- **Godot's front face for triangle primitives is CLOCKWISE under `CULL_BACK`**, which is
  what Godot's own `Mesh` documentation says. The mesher emits its `A→B→C→D` quad walk
  reversed to achieve it. (The code previously named the constant `CLOCKWISE_FRONT =
  false` and behaved as though the front face were clockwise: the geometry was right and
  the recorded fact was inverted.)
- **The padded neighbour copy does not eliminate T-junctions.** It stops faces that are
  interior across a chunk boundary being emitted, which is a different thing. Greedy
  meshing produces T-junctions by construction wherever one merged quad abuts several
  smaller coplanar ones, inside a chunk as much as across a seam. None was visible here
  because every vertex coordinate is a small exact integer in `f32` and MSAA is off — a
  measurement on this configuration, not a guarantee. Re-check it if the surface format
  ever gains interpolated attributes or the chunks stop sitting on integer transforms.

### 10.8 Step 7 — the headless CPU proxy

`meshbench --pipeline --explosions-per-s 20 --frames 3600 --k 4 --b 524288
--upload-rate 1353700` runs the real dirty set, the real drain policy and the real
mesher, and models only the upload. The rate given is path B's measured 77 025 B / 56.9 µs;
the proxy charges the **FFI** byte count (96 623 B/chunk) against it, so the modelled
upload is ~1.25× slower than path B actually achieves — conservative, in the direction
that can only make the latency worse. (The committed CI workflow uses 1 740 000 B/ms,
derived the other way round as 96 623 B / 56.9 µs; this run re-measures that constant at
1 698 000, within 2.5 %, so the committed budget still holds.)

```
frames 3600 at 20 expl/s, K=4 B=524288
explosions 1200  chunk uploads 5746  bytes 555194568
  remesh latency frames: p50 0 p90 1 p99 1 max 2   (G1-a wants p99 <= 3)
  modelled CPU per frame us: p50 0 p99 2365 max 2605   frames over 16.67 ms: 0/3600
  max queue depth 8  coalesced 0
```

**The proxy reproduces the in-engine latency series exactly** — p50 0, p90 1, p99 1, max 2,
max queue depth 8, 5 746 uploads — which is the strongest thing that can be said for it:
the queue policy, not the GPU, is what sets G1-a at this load, and a CI job with no GPU
can therefore catch a regression in either the mesher or the drain policy. It is still a
proxy and its frame number is modelled, not measured; it is labelled as such in the
output and must never be quoted as a G1-c result.

### 10.9 The gate

| | required | measured (Windows / Vulkan / Quadro P4000, path B, 3 reps) | |
|---|---|---|---|
| **G1-a** latency | every dirtied chunk visible within 3 frames | p50 0, p90 1, p99 **1**, max **2** frames in all six headline runs; 0 chunks left queued; the headless proxy agrees exactly | **pass** |
| **G1-b** load | sustained 20 explosions/s, not a burst | 1 200 explosions over 3 600 frames, three repetitions per path, 5 746 chunk remeshes each; the schedule is frame-driven, so with vsync off the wall-clock rate was 209–220 explosions/s | **pass** |
| **G1-c** frame rate | frame time p99 ≤ 16.67 ms | p50 0.64–0.70 ms, p99 5.52 / 6.29 / 5.65 → **5.65 ms** (median of 3), max 11.02 ms; **0 of 3 600 steady-state frames over budget**, and the single over-budget frame in the all-frames column is frame 2 (17.8–21.6 ms), the first rendered frame | **pass** |

Worst case across *both* paths and all six headline repetitions: p99 6.29 ms, max
11.16 ms, 0 steady-state frames over budget. There is roughly **2.6× frame-time headroom
at the p99** (and 1.5× at the worst single frame) and the latency budget is used at one
third of its allowance. At 40 explosions/s — twice the gate — latency p99 reaches exactly
3 frames, so the gate is met with a factor of two in load headroom as well.

### 10.10 Verdict

> **GO — on Windows/Vulkan only. The cross-OS half of the gate is outstanding.**

Plan §3 step 8 requires steps 3–5 repeated on Linux on a real GPU with "the gate judged on
the worse of the two", and it **was not run**: this machine has no Linux GPU and no Linux
runner was available. Plan §6 additionally asks for D3D12 recorded separately on Windows;
that was not run either. Step 9's per-OS screenshot exists for Windows only.

So: **the verdict covers Windows/Vulkan/Quadro P4000 and nothing else.** The margins are
large — 2.6× on frame-time p99, 3× on latency — so a Linux result would have to be
dramatically worse to overturn it, but that is an expectation and not a measurement. The
owner should either schedule the Linux half or explicitly accept a single-platform result
**before** this is written into `docs/design/decisions-log.md` §2.7.

Not run, or run differently from the plan, in full:

- **step 8: Linux/Vulkan repetition of steps 3–5**, and step 9's Linux screenshot. Not
  run at all.
- **plan §6: D3D12 on Windows, recorded separately.** Not run.
- **plan §6: the Windows-Defender-exclusion comparison.** Measurement ran with real-time
  scanning enabled and no A/B was taken, so the numbers include whatever it costs.
- **plan §6: an alternative allocator comparison.** Not needed — the mesher allocates
  0.001 times per meshing, so there is nothing for an allocator to be slow at.
- **The CI workflow has not been run as a workflow, on any machine.** The `mesher-cpu`
  job's commands have all been executed here (`meshbench --json`, `meshbench --pipeline`),
  and the `geometry` job's assertion has now been executed *by hand on Windows/Vulkan*
  (§10.7) rather than under xvfb + lavapipe on Linux, which is the configuration whose
  tolerances actually need proving. The `frame-time` job is `if: false` by design.
- **Frame time is measured at ~650 fps, not at 60.** The plan asks for vsync off, which
  is what makes the frame time a measurement rather than a display reading; the
  consequence is that the explosion schedule, which is defined in frames, runs ~10.5×
  faster in wall-clock terms than the gate's 20/s. Harsher on load, silent on 60 Hz-specific
  driver behaviour. A vsync-on confirmation run at a true 60 Hz was not taken.
- **The plan's `process_ms` / `physics_ms` CSV columns are not per-frame values** and are
  recorded under the names `process_ms_1s_max` / `physics_ms_1s_max` instead; see §10.12.

### 10.11 Decision candidates (plan §9), for the owner to approve

Plan §9 asks this spike for one decision in four parts. Each part below gives the
candidates that the measurement actually leaves open, the recommendation, and what it
would cost to take the other one. Nothing here has been written to
`docs/design/decisions-log.md` §2.7 — that is the owner's to do.

**(1) How voxel geometry reaches the screen: ArrayMesh or `RenderingServer` RIDs.**

| | path A — `ArrayMesh` + `MeshInstance3D` | path B — `RenderingServer` RIDs |
|---|---|---|
| upload cost/chunk | 84.0–84.5 µs | **56.8–57.4 µs** (1.48× cheaper) |
| bytes/chunk | 96.6 KB | **77.0 KB** |
| resource churn | 5 798 `ArrayMesh` objects per minute | **none** (241 RIDs, reused) |
| frame-time p99 | 4.69 ms (median of 3) | 5.65 ms (median of 3) — **a tie; see §10.4** |
| gate rows | all pass | all pass |
| complexity cost | none; inspectable in the Godot editor | manual RID lifetimes, a probed colour conversion, a format that must match exactly, an index buffer that cannot be patched |

- **Recommended: path B.** The two clean, repeatable measurements — per-chunk upload cost
  and resource churn — both favour it, and neither overlaps between paths.
- **Honest counter-candidate: path A.** It passes every gate row, its geometry is
  byte-identical, and the ~27 µs/chunk it costs is ~43 µs/frame at the measured 1.6
  uploads per frame — 0.3 % of the frame budget, and smaller than the run-to-run noise.
  What path A buys for that is the absence of four classes of bug that path B has to keep
  paying for in the skeleton (dangling RIDs that silently draw nothing, a surface format
  probe, a hand-written index-equality guard, a colour quantisation that must match Godot
  byte for byte — one of which, the index guard, this spike hit and would have shipped
  back-face-culled holes without).
- **A defensible third option the owner may prefer: take path B, but keep path A alive as
  a build-time switch in the skeleton's bridge**, since both were written here in a day
  and the geometry is provably identical. That makes "retreat to A" a flag rather than a
  rewrite.

**(2) The per-frame upload budget.**

- **Recommended: *K* = 4 chunk surfaces, *B* = 512 KiB per frame**, drained
  nearest-to-camera first, ties by chunk index, with anything older than 2 frames jumping
  the queue. A chunk re-dirtied while queued is coalesced and keeps its *earliest* issue
  frame. The first chunk of a frame always goes through even if it alone exceeds *B*, so
  an oversized surface cannot stall the queue.
- *K* = 4 is the **smallest** *K* that meets G1-a (latency p99 1, max 1, nothing left
  queued). *K* = 1 and 2 fail it outright — 65 and 12 chunks respectively never got
  uploaded at all in 900 frames. *K* = 8 and unbounded also pass; see §10.5 deviation 2
  for why the smallest passing *K* was chosen and why **no measured frame-time reason**
  supports that choice on this machine.
- *B* = 512 KiB is one doubling above where *B* stops binding (256 KiB), i.e. a margin
  against a heavier future vertex format, not an optimum.
- **The owner should ratify the amended rule** ("smallest *K* meeting latency", replacing
  the plan's "largest *K* meeting frame time"), because the plan's rule as written selects
  "unbounded" and therefore selects no budget at all.
- **Open:** the ageing term never fired in any measured run (max queue depth 8, latency
  max 1–2). It is insurance against a policy that is unbounded by construction — pure
  distance ordering lets a far chunk be deferred indefinitely — and is therefore untested
  code. Keeping it means carrying an untested branch; dropping it means shipping a drain
  order with a known starvation mode.

**(3) Do 32³ chunks and single-voxel destruction survive?**

- **Recommended: yes, unchanged. No rung of the fallback ladder is taken.** Fan-out is
  capped at 8 chunks per explosion, mesh cost is ~450 µs/chunk, 16-bit indices suffice
  with 10× headroom (6 660 vertices max against 65 536), peak heap is 18.8 MiB and video
  memory 82.9 MiB. Nothing is under pressure.
- **The only caveat worth recording:** the headroom claim is Windows/Vulkan-only (§10.10).
  If the Linux half ever runs and is materially worse, rung 1 (coarser destruction, 2³
  edit groups) is the cheap lever and it does not touch the sim's voxel grid.

**(4) Main thread or worker pool, and where the presentation wall sits.**

- **Recommended: the mesher runs on the main thread.** That is a measurement, not a
  preference: 450 µs per chunk × *K* = 4 is 1.8 ms of a 16.67 ms frame, and the measured
  frame p99 is 5.65 ms. A worker pool is not needed for v1's load and would cost the
  determinism-adjacent complexity of a second thread touching world state. Revisit if the
  world grows or *K* has to rise — 40 expl/s already wants a larger *K*.
- **The wall: G1 hands the harness a *line*, not a mechanism, and the mechanism is a
  contract-file change the owner has to approve.** The line is exactly this: everything up
  to and including the voxel edit, the dirty set and the drain order is integer; floats
  start at the first vertex coordinate, and nothing a float touches is ever read back by
  the sim. In the spike this is enforced by a per-file attribute — `chunk.rs`, `world.rs`,
  `explode.rs`, `budget.rs` and `stats.rs` each carry a literal
  `#![deny(clippy::float_arithmetic)]`, while `greedy.rs`, `upload_arraymesh.rs`,
  `upload_rs.rs` and the gdext node are the walled side. **That shape must not transfer.**
  AGENTS.md §4.9 is explicit that the wall has to be a crate boundary: a per-file
  `#![allow]` inside a deterministic crate is invisible to `cargo xtask wall-guard`, which
  can only see dependency edges. The candidates for the skeleton are therefore:
  (a) a `mesher` crate on `WALLED_PACKAGES` that the client depends on and the sim does
  not — the shape AGENTS.md §4.9 already anticipates; or (b) keeping the mesher inside
  `client-gdext`, which is already walled, at the cost of the sim-side crates never being
  able to reuse it. Either way the change to `WALLED_PACKAGES` and `clippy.toml` is a
  §5 contract change and needs owner approval before harness part 1 writes the lint set.

### 10.12 Things worth carrying forward that are not the decision

- **The flood fill must seed every sky cell, not the lowest one per column.** Seeding only
  `(x, height, z)` leaves any air cell whose only lit neighbour is a sky cell above an
  adjacent column's floor at light 0 — a tunnel mouth in a cliff, a breached bunker,
  anything under an overhang renders pitch black, and the whole world's `(material, light)`
  mask key degenerates to a near-uniform 15 so the mesher is never exercised on
  light-driven quad fragmentation. Seeding all of them costs 6.5 M queue entries on this
  world; seeding only those with a taller horizontal neighbour is exactly equivalent and
  costs a handful per column.
- **The dirty pad is `light reach + 1`, and the two terms compose rather than compete.**
  The mesher reads the *neighbour* cell one voxel outside the chunk, so a light change at
  the edge of its reach still alters a face one voxel further out. Both constants are
  derived from `LIGHT_MAX` and `LIGHT_ATTEN` rather than asserted, because the old value
  happened to be right for the wrong reason and would have gone one short the moment the
  attenuation changed.
- **`Performance.TIME_PROCESS` and `TIME_PHYSICS_PROCESS` are per-second maxima**, refreshed
  once a second, not per-frame values — an 18 ms "process time" logged on a 1.4 ms frame is
  a stale per-second peak, not a contradiction. The CSV columns are named
  `process_ms_1s_max` / `physics_ms_1s_max` so nobody reads them as per-frame again.
- **`godot --headless` cannot take a screenshot.** It selects the dummy rendering driver,
  `RenderingServer.frame_post_draw` never fires, and a screenshot coroutine awaiting it
  parks for ever — silently, producing no PNG and no error. A CI job that runs Godot
  headless and uploads "the screenshot" as an artefact checks nothing at all. The software
  rasteriser plan §4 asks for (lavapipe under xvfb) is a different thing from `--headless`.
- **A screenshot costs a frame and must never land inside a measured series.** The two
  vista runs are the only ones with a steady-state frame over 16.67 ms: 90.7 ms (path A)
  and 67.2 ms (path B) on the shot frame, from the image read-back and PNG encode. Every
  headline and sweep run is taken with `--shot=0`.
- **gdext's panic catch at the `#[func]` boundary is silent.** A panic in the mesher
  becomes a default return value and a healthy-looking CSV. The extension counts its own
  caught panics and `analyse.py` fails on a non-zero count; the skeleton's bridge should do
  the same rather than trust the boundary to be loud. Measured: **0 panics across all 25
  runs.**
- **Two builds, two target directories.** `--no-default-features` rebuilds the lib target,
  so building `meshbench` into the same target directory silently replaces the staged
  cdylib with one that has no `gdext_rust_init` in it; Godot then reports "Can't resolve
  symbol gdext_rust_init, error 127", which reads like an ABI problem and is not one.
  `build.ps1` gives the bench its own target directory and byte-scans the staged DLL for
  the export before declaring success.
- **A `foreach ($k in …)` loop and a `$K` constant are the same variable in PowerShell.**
  Variable names are case-insensitive, so a sweep loop silently overwrites the chosen-value
  constant and the runs after it use the loop's last iteration. That cost one whole
  measurement set. `runall.ps1` uses `$CHOSEN_K` / `$sweepK` and echoes the effective
  *K*/*B* of every run for exactly this reason.
