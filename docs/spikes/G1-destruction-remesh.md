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
  Cargo.toml            # [workspace] (empty) + cdylib; godot = "PLACEHOLDER" (gdext for Godot 4.7)
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
    project.godot       # Godot 4.7; vsync off; Forward+ ; window 1920x1080
    main.tscn           # camera at a fixed vista pose, the world node, a CSV logger
    main.gd             # drives the run: N frames, 20 explosions/s, writes frame-time CSV, quits
```

**What the mesher does.** Greedy meshing on a 32³ chunk: for each of the 6 face directions, sweep the 32 slices, build a 32×32 mask of visible faces keyed by `(material, light)`, and merge each maximal rectangle into one quad. One surface per chunk with a palette material; colour and the flood-fill voxel light go into vertex colours, as section 15's art-pipeline row describes ("flood-fill voxel light baked into vertex colours on remesh"). Neighbour-aware at chunk borders: a chunk needs its 6 neighbours' boundary slices to avoid meshing faces that are actually interior, which is why one explosion straddling a border dirties more chunks than it visually touches.

**Both upload paths are built**, because choosing between them is one of the spike's outputs:

- **Path A — ArrayMesh.** Build a `PackedVector3Array`/`PackedColorArray` set, `ArrayMesh.add_surface_from_arrays`, assign to a `MeshInstance3D`. Idiomatic, debuggable in the Godot inspector, costs a trip through Godot's Variant/Packed array marshalling and allocates a new mesh resource per remesh.
- **Path B — direct RenderingServer.** `RenderingServer.mesh_create()` once per chunk to get a **RID**, `mesh_add_surface_from_arrays` on first build, then `mesh_surface_update_vertex_region` / `mesh_surface_update_attribute_region` for in-place updates when the vertex count is unchanged, with `instance_create2` + `instance_set_transform` + `instance_set_scenario` for placement. No scene-tree node per chunk, no resource churn — but the surface format must match exactly on update, the RIDs must be freed by hand, and the calls must be made on the rendering thread (`RenderingServer.call_on_render_thread` when the render thread is separate).

**The per-frame upload budget** is the knob the spec already assumes exists. Implement it as a bounded queue: the sim marks chunks dirty; each frame the drain loop uploads at most *K* chunk surfaces and at most *B* bytes, whichever binds first, taking chunks in a deterministic order (nearest-to-camera first, ties by chunk index). *K* and *B* are what the spike measures. A chunk re-dirtied while still queued is coalesced, not queued twice.

**Determinism note.** Mesh output is presentation, not hashed state: the mesher's vertex data is floats and lives on the far side of the wall section 15 draws around presentation. The *input* — which voxels an explosion removes — is integer and deterministic and belongs to the sim. The spike must keep that line visible, because the float-arithmetic lint has to carve out exactly this one stage and no more. Getting the carve-out wrong is how floats leak into the sim later.

## 3. Measurement procedure

**Step 0 — Prerequisites.** Install Godot 4.7 (`PLACEHOLDER` — exact patch version at day 0) and the Rust toolchain with the **MSVC** ABI on Windows; gdext cdylibs must match the ABI Godot was built with. Record both versions and the GPU.

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

1. **The CPU half is measured headlessly and is CI-gradeable.** `meshbench` (step 1) is a plain Rust binary with no Godot at all: mesher cost per chunk, fan-out per explosion, bytes per surface, allocations. Add a second headless binary that simulates the *budgeted pipeline* — issue 20 explosions/s of virtual frames at 16.67 ms, run the real dirty-set and the real drain policy, and compute the latency-in-frames series with upload cost modelled as `bytes / PLACEHOLDER_upload_rate` measured once in step 3/4. This gives a CI-stable proxy for G1-a that catches a regression in the mesher or the queue policy, and it is honest about being a proxy.
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
      # assert p99 chunk mesh time <= PLACEHOLDER us (set from the day-2 measurement,
      # then held as a regression budget) and bytes/surface p99 <= PLACEHOLDER KiB
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

*(Empty until the spike runs.)*

- Machine: `PLACEHOLDER` (CPU, RAM, **GPU and driver version**, OS build, refresh rate)
- Godot: `PLACEHOLDER` · gdext: `PLACEHOLDER` · Rust: `PLACEHOLDER` · rendering driver: `PLACEHOLDER`
- G1-a remesh latency (frames, p50/p99/max), Windows / Linux: `PLACEHOLDER`
- G1-b sustained explosions/s held: `PLACEHOLDER`
- G1-c frame time (ms, p50/p99/max) and frames over 16.67 ms: `PLACEHOLDER`
- Path A vs path B: `PLACEHOLDER`
- Chosen *K* / *B*: `PLACEHOLDER`
- Verdict: `PLACEHOLDER` (go / which rung of the fallback ladder)
