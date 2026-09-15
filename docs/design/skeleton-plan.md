<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# Walking skeleton — plan for spec section 17, the wk-18.5 row (15.5 weeks)

**PROPOSED — for the owner's approval. Nothing in section 6 is decided; nothing in section 3 starts
until the decisions in section 6 that gate it are answered.**

Precedence followed throughout: `docs/design/decisions-log.md` §2.7 > `docs/spec/pharmakos-spec-v0.6.html`
> `docs/design/co-design-gameplan-api.md` (`docs/design/README.md`). Where the two higher sources
disagree with each other, the conflict is named and put to the owner in section 6 or 7 rather than
resolved here. Where the spec is silent, nothing is invented: it becomes an owner decision with a
recommendation and the downsides of every option.

**Ordering principle.** A task earns its place by how expensive a wrong shape is to unpick later,
not by how visible it is — the sim's hashed state, the proto envelope, the estimator's contract, the
gateway's security surface, the mesher seam and the golden formats all come before anything that
merely adds content, and each closes with a golden or a measurement rather than a demo. Two things
are pulled forward against that principle on purpose: harness part 2's *formats* (section 5), so
the definition of done is a command from the first merged PR, and the vista and watch rig, so the
owner has something to look at from about week 8 rather than week 12.

**Baseline.** This plan sits on top of the in-flight harness PR on `feat/harness-mesher-crate`
(decisions-log items 56, 70 and 71): it adds `crates/mesher` to the workspace and to
`WALLED_PACKAGES`, updates `clippy.toml`'s header and the AGENTS.md crate map, removes the §3 `math`
placeholder, retires `spikes/` and the four `spike-*.yml` at the `spike-end` tag, and corrects
CLAUDE.md's pre-toolchain note. The toolchain **is** installed and `cargo xtask ci` **is** green
locally and on the three-OS matrix as of 2026-09-14 (handoff, "Build state"); every task below
compiles its own work and runs `cargo xtask ci` before its PR opens. Spike code is read at
`git worktree add ../pharmakos-spikes spike-end` and never copied into `crates/`.

---

## 1. Goal and end state

### 1.1 The demo the owner reviews

One unsigned Windows zip (the same build on Linux) opens into a lobby. The owner starts a
Probation-shaped match — two seats, themselves and one Easy built-in operator, three rounds on the
per-round length list 3 / 5 / 8 minutes — on a seeded map about 384 × 384 × 64 whose two occupied
spawn zones sit at least the spawn-distance rule apart, each with a pre-placed fortified core beacon
on a Build mandate, a commander, two build drones, a mining drone, a reachable heat vent a short
walk away and `$` equal to two rounds of BMI.

The first Lull opens the editor wizard on three templates. The owner fills a parameter or two,
watches the route draw as a polyline with a travel time on each leg (fogged legs dashed and labelled
as a bound, never as an ETA), sees QUICK diagnostics appear on every edit as icons and plain
sentences with Fix buttons, writes a line in the notes box, and submits. The verifier runs FULL,
`render_plan` prints the playbook as English prose, and the `report_hash` at submit equals the one
from the FULL pre-check. The built-in operator has already filed the other seat's playbook, and
would have filed the safe playbook for the owner had the timer run out — which the editor can render,
so the cost of a timeout is visible.

Ready ends the Lull. The Push runs three minutes with nobody in control, watched through one own-fog
camera with free-look and a follow-commander toggle, a live event list, 2–4× speed and a free instant
skip to segment end. The commander walks a real route, interfaces on site to add a build target,
places a beacon, a Generator goes up on the vent, kW supply rises, the Quartermaster stub holds
fabricator orders when draw outruns supply, ore is credited to the single treasury on delivery, and
voxels the demo edits remesh in the vista inside the K = 4 / B = 512 KiB per-frame budget. The recap
settles BMI by standing. The next Lull pre-loads last round's playbook as a re-verified draft, and
the host can save from that frozen snapshot and resume from the lobby. The match ends on the round
limit or the one-tick rule, with the full map unlocked.

### 1.2 The machine-checkable half — AGENTS.md §10, item by item

| §10 item | What closes it at the skeleton | Task |
|---|---|---|
| 1. `cargo xtask ci` green on the CI matrix, all three OSes agreeing on the hash chains | Every step real rather than skipped; the three-OS comparison job that replaces the deleted spike workflows | T20 |
| 2. Determinism hashes committed, every movement explained in the PR that moved it | `tests/golden/determinism/expected.hashes.txt` from T2 onward; each sim PR that extends hashed state explains the movement | T2, T5, T7, T10, T11, T14, T17 |
| 3. Golden playbooks round-trip byte-identically, verify identically (same `report_hash`), render identically | `tests/golden/plan-core/**`, `tests/golden/verifier/**` | T6, T8 |
| 4. Headless scenario runs pass on assertions over events **and** hashes | `gamectl scenario run` over `scenarios/**`, wired into `xtask ci` | T3, T15, T20 |
| 5. The three nightly adversarial scenarios | **Not this stage** — they start at S2 (spec section 15). `nightly-scenarios.yml` stays gated off | — |
| 6. The stage's named depth task and its gate, or a deliberate written fallback | The skeleton has no named depth task in section 17's row; gates P1, P6, P2, G3′-real belong to S1, S3, S5 and S2. What this stage owes is that each of those gates' *shape* exists and is measured as a baseline, not certified | T6, T7, T19 |
| 7. Docs updated in the same PR | Every task states which doc moves with it | all |
| 8. The owner reviews the stage demo | §1.1 | T22 |

### 1.3 What "ends playable" means here, precisely

Everything in section 17's wk-18.5 row and nothing else: sim and Godot vista · JSONC playbooks ·
Seat Gateway · verifier QUICK · interpreter at the v1 vocabulary · editor wizard with 3 templates ·
built-in operator Easy and the safe playbook · Survey-lite · notebook · minimal Build mandate
(Generator blueprint, 2 starting build drones) with a single-treasury Quartermaster stub · `$` with
BMI, `kW` with core surplus and Generators · the watch rig · seeded map with the spawn-distance rule ·
harness parts 1 and 2 · about 1 wk of packaging.

Two additions are proposed beyond that list, each as an owner decision rather than as scope taken
quietly: **save and resume at Lull boundaries** (spec section 3 and section 15 both specify it, the
wk-18.5 row does not name it — decision 13), and **the private deterministic replay** the scenario
runner and the recap read (spec section 15, "Replays"; the *shareable recording* format is S7 and is
not built). Everything else the earlier drafts of this plan proposed beyond the row has been cut.

---

## 2. What the spikes fixed, and where each decision lands

Items 48–71 are closed. This table is the contract between the spikes and this stage: an agent who
wants to do something differently from a row here is re-opening a decision, which is section 5 of
AGENTS.md, not a judgement call.

| Item | What it fixed | Where it lands |
|---|---|---|
| 46 *(not a spike item)* | `int32` game milliseconds for every duration in `gp.v1`; negatives rejected by the verifier with a pointer, not at decode | T1 (fields), T6 (the rejection) |
| 47 *(not a spike item)* | `Playbook` field 7 / name `kind` reserved, "defined with the skeleton" | T1, decision 5 |
| 48 | Canonical little-endian encoding in id order, `u32` lengths, fixed-stride `Option`, `ENCODING_VERSION = 1` first, xxh3-64 under `STATE_HASH_SEED = 0x5048_4152_4D4B_4F53`; pinned `digest(b"")` and `digest(b"pharmakos/g4")` | T2, as tests |
| 49 | postcard 1.1.3 (`default-features = false`, `use-std`), fixed-width fields, sentinel `Option`, `SNAPSHOT_VERSION` in the bytes and checked on restore; the LEB128 `Vec`-length bend documented at the encoder | T2; save/resume in T17 |
| 50 | Counter-based stateless RNG, `GOLDEN = 0x9E3779B97F4A7C15`, SplitMix64 finaliser; stream ids `Combat = 1`, `Spawn = 2`, `Map = 3`, additive only | T2; `Map` used by T5 |
| 51 | The determinism rule set (no floats, no `as`, no `HashMap`/`HashSet`, no clock, overflow checks on in every profile, every sort key ends in a unique id, no recursion) | Already in `[workspace.lints]` and `clippy.toml`; T2 adds the **source-text confinement test** G2 and G3′ both recommend copying |
| 52 | The single-platform frame-time GO is accepted; the Linux frame-time half stays unmeasured until a GPU runner exists | T20 leaves the `frame-time` job `if: false`; nothing in this stage claims a Linux frame number |
| 53 | Path B (`RenderingServer` RIDs) with path A (`ArrayMesh`) kept as a build-time switch, and the four guards that are the price of path B | T12 |
| 54 | K = 4 surfaces / B = 512 KiB per frame, drain order (older than 2 frames, then nearest-camera, ties by chunk index, coalesce at the earliest issue frame, first chunk always through), as rules-table data | T4; the rules-table home is decision 7 |
| 55 | 32³ chunks, single-voxel destruction, no rung of the G1 fallback ladder | T4, T5 |
| 56 | The mesher runs on the main thread in its own walled crate `crates/mesher` | T0 (the crate), T4 (the code) |
| 57 | The editor promises a travel time as a number; short routes rounded generously or in whole seconds; a fogged leg drawn as the bound it is | T7 (the estimator), T8 (the rendering rules), T19 (the drawing) |
| 58 | Cluster 32 = one chunk footprint, a single abstract level; the lever if the estimate budget bites is the **endpoint sweep** (91 % of a median query), not the abstract graph | T7; the endpoint-sweep share is instrumented in T7's bench so S3 inherits the number |
| 59 | `STEP_CARDINAL = 10`, `STEP_DIAGONAL = 14`, `CLIMB_SURCHARGE = 4`, `MOVE_COST_PER_TICK = 3`, cost→ticks by **ceiling**, all four as rules-table data stamped into the rules hash | T7 |
| 60 | Eager chunk-footprint repair, one CSR + connectivity rebuild per tick, a per-tick repath cap in round-robin `(seat, beacon, unit)` order, and **"sealed in" as a designed state** — park and report, never a repath loop | T7 |
| 61 | `estimate(...) -> Option<Estimate { cost, ticks, legs }>`; `None` from the connectivity oracle before any search; abstract search only — never refines, steps or forks; ±15 % p90 on static terrain and **never optimistic**; fog priced `cost * 3 / 2` per abstract edge; **no cache** | T7 (the function), T8 (the pass-through), T13 (`estimate_route`) |
| 62 | Search nodes ordered `(f, h, node_id)` in a hand-written binary min-heap — part of the determinism contract | T2 states the convention crate-wide; T7 implements it |
| 63 | "4 seats, 300 units, 40 beacons" means **world totals** | T5's power ceiling and T14's costs are sized against world totals |
| 64 | The five-number provisional tick cost model (`a0` 1 022 ns, `d` 1 234 ns, 6 960 ns per voxel edit, 1 041 ns per decision tick per seat, 0.47 ms fixed — which is G2's CSR rebuild, so S2 must not charge it twice) | T14's bench reports against this shape; **no budget is gated** at this stage |
| 65 | Provisional per-map power budget 190 kW at 20 craters/s, with 1 kW per unit and the 40 % reserve as PLACEHOLDERs inside it | T5 (the generator's ceiling), decision 15 |
| 66 | **Full** per-tick state hash; the chunk store enters as per-chunk digests, never as bytes | T2 (the hash), T5 (the digests) |
| 67 | Vec-backed SoA tables with counts fixed at construction; CSR uniform grid rebuilt by counting sort; the caveat that the broadphase is super-linear and its cell size is a density-tied tuning value | T2 |
| 68 | The 3 / 5 / 8 ladder is affordable; keep it | T10 (segment lengths from the snapshot, not a constant) |
| 69 | Per-tick repath cap **16**, re-derived at S2's exit once the burst frequency is a measurement | T7, as a rules-table value with the re-derivation date in its comment |
| 70 | **No `math` crate** — the newtypes, RNG, encoding, hash, HPA\* and the map generator stay as modules inside `pharmakos-sim` | T2, T5, T7; the contingency if the sim slips is decision 21 |
| 71 | `spike-end` tagged, `spikes/` deleted in the item-56 PR; what survives of the spike workflows is re-expressed inside `cargo xtask ci` as the corresponding checks land | T0 (the deletion), T2/T7/T4 (the checks), T20 (the cross-OS jobs) |

### 2.1 Out because a spike measured it and said not to

Each of these is closed by a number, and re-opening one from intuition is how a determinism hole
gets optimised into the artefact. No task in this plan builds any of them.

- **A second abstract pathing level.** Endpoint insertion is 91 % of a median query, so the ceiling
  on a second level is 9 % (item 58).
- **An estimate, route or per-unit path cache.** P3-a passes with 18 % headroom, and a cache is
  hashed state that must invalidate identically on three platforms (item 61, G2 §9.11).
- **An incremental state hash.** 7.641 of 8 tables are dirty on essentially every tick; the saving
  is 0.02 % of the gate tick, bought with a dirty flag on every write site in the sim, for ever, on
  the determinism artefact itself (item 66, G3′ §9.17).
- **A worker-pool or threaded mesher.** 1.8 ms of a 16.67 ms frame does not need one, and a second
  thread near world state is the wrong kind of clever (item 56).
- **A two-level or hierarchical broadphase.** A solution to a problem the gate load does not have at
  0.22 ms, and a second structure to keep deterministic and hashed (item 67).
- **`imbl` persistent maps in hot tables.** Small-allocation traffic back in the hot loop (item 67).
- **The corner entrance per cluster.** Carried to S3 by G2, recorded and not built.
- **A `math` crate.** Item 70 — it would add a crate without removing a dependency edge, and give
  the determinism contract a second home.

---

## 3. The tasks

Conventions for every task: its own worktree (`git worktree add ../pharmakos-<id> -b <type>/<crate>-<id>`),
**one crate, one agent** (AGENTS.md §6), rebase on `main` before the PR, conventional-commit subject,
`git commit -s`, SPDX header on every new file, `cargo xtask ci` green on the branch before the PR
opens. "Contract PR: yes" means the task touches an AGENTS.md §5 path — the agent opens the PR and
**stops**; the owner merges. Estimates are agent-days including the agent's own iteration to green
CI; they do not include the owner's review latency, which the float in section 4 is for.

**Two standing rules, both grafted from the losing plans because they fix real failure modes:**

1. **`xtask` and `.github/workflows/**` belong to no wave agent.** Changes to them arrive as separate
   single-purpose PRs (T3, T15's step fill, T20, T21), and **at most one is open at a time**. Contract
   PRs serialise on one reviewer; this is the only lever that schedules for it.
2. **Every task over 10 agent-days carries a named split seam, written before it starts.** A task that
   is going long is split at that seam into two sequential PRs by the same agent, rather than arriving
   as one diff a solo reviewer cannot hold in their head.

---

### T0 — harness PR: `crates/mesher`, `WALLED_PACKAGES`, retire `spikes/` *(in flight)*

- **Crate(s) owned:** `xtask`, new `crates/mesher`, `clippy.toml`, `Cargo.toml`, `AGENTS.md`, `CLAUDE.md`, `REUSE.toml`, deletion of `spikes/**` and `.github/workflows/spike-*.yml`.
- **Builds:** the walled `pharmakos-mesher` crate (documented, `mesh_chunk` a stub), its entry in `WALLED_PACKAGES` and in `clippy.toml`'s header, the AGENTS.md crate-map row and §4.9 list, removal of the §3 `math` placeholder, the corrected CLAUDE.md toolchain note, and the deletion of the spike tree behind the `spike-end` tag.
- **Implements:** items 56, 70, 71.
- **Needs:** nothing.
- **Acceptance:** `cargo xtask ci` green on the branch, `wall-guard` proving no deterministic crate reaches `pharmakos-mesher`, `reuse` green; `spike-end` pushed before the deletion commit.
- **Contract PR:** yes.
- **Agent-days:** 2.
- **PLACEHOLDERs:** `clippy.toml` narrows to "`client-gdext` and `mesher` of the four exist" (owner, if a `presentation` or `solve` crate is ever proposed — or never).

---

### T1 — `crates/proto`: the envelope, the vocabulary, the codegen and canonical JSON

- **Crate owned:** `crates/proto` (+ `proto/**`, `buf.*`).
- **Builds:** `Playbook.kind` on reserved field 7 with its enum (item 47's deferral, discharged); the plan fingerprint's definition; the stub messages the skeleton actually uses filled in — `InterfaceRow` and the section-5 row catalogue, `Location`, the four selector forms (`nearest`, `weakest`, `safest`, `most_threatened`), `MandateSettings` with `BuildSettings`, `SurveySettings` and `MineSettings`, `Options`, `OnDeath`, `Fallback`, and the condition families the three templates and the v1 vocabulary need; every deferred construct left `reserved` **with the version it returns in** (`set_flag`, `clear_flag`, `branch`, `repeat`, flag predicates, `dispatch`, `team_id`, capture, logistics, radio predicates); the `gp.api.v1` request/response messages for the skeleton's method subset, with the scope table as a machine-readable per-method annotation; the prost codegen path writing a **checked-in** tree under `crates/proto/src/generated` (so a plain `cargo build` needs neither `buf` nor `protoc`); the canonical proto-JSON codec, encode *and* decode, which is the disk format the whole project rests on; and `gp.v1.RulesTable` if decision 7 takes that shape.
- **Implements:** spec sections 10 (format, vocabulary, versioning, reserved seams), 11, 12 (method list, error set, scopes), 15 (Schema, Seams kept for later); items 46, 47.
- **Needs:** T0.
- **Acceptance:** `buf lint` clean; `buf breaking` against `main` in `WIRE_JSON` mode green from the workspace root; a proto → canonical JSON → proto round-trip test for every message; `examples/playbooks/expand_east.jsonc` decodes with **zero unknown fields**, re-encodes to bare-number durations, and is committed as `tests/golden/proto/expected.expand_east.json`; a test that an unknown field is *rejected*, not ignored; a table test that every reserved number is still reserved; a test that no `int64` duration field exists in `gp.v1`.
- **Contract PR:** yes. This is the PR that must not churn afterwards.
- **Agent-days:** 8.
- **PLACEHOLDERs:** `kind` enum membership (decision 5, before merge); the fingerprint's hash function (decision 6); JSON-RPC parameter casing (decision 9); the remaining stub catalogues stay `PLACEHOLDER STUB` naming the stage that fills each — Defend and Attack at S2, broadcast bodies at S4, the full predicate catalogue at S3.

---

### T2 — `crates/sim` part 1: the determinism core, and the public type surface on day one

- **Crate owned:** `crates/sim`.
- **Builds:** the integer newtypes as modules inside the sim (item 70) — `Fx` Q16.16, `Sq` Q32.32, `Angle` u16 + 4096-entry LUT, `Hp`, `Money`, `Kw`, `Tick`, `Ms`, each with named constructors and the handful of audited widening casts carrying their `#[allow]` and a comment; the counter-based RNG with the stream enum; the canonical encoder (item 48); the **full** per-tick state hash with the chunk store entering as per-chunk digests (item 66); postcard snapshot/restore with `SNAPSHOT_VERSION` (item 49); Vec-backed SoA tables with counts fixed at construction and the CSR uniform grid rebuilt by counting sort (item 67); the named tick phases in fixed order, most of them empty stubs naming the task that fills them; the rules-table loader and `rules_hash`; the in-code seams spec section 15 names (the `Operator` trait, the abstract per-tick work counter, `program_id` on a beacon's mandate); `fork` behind `feature = "research"` and nowhere else; and the `determinism` binary `xtask/src/main.rs` is already waiting for (`DETERMINISM_BIN`, `HASH_GOLDEN`, `DETERMINISM_TICKS = 1200`, profile `release-checked`).
  **And, delivered first inside the task:** the crate's **public snapshot, knowledge and rules types** under `default-features = false`, so `plan-core`, `verifier`, `gateway` and `operator` never queue behind the sim's critical path again. This is the cheapest defence in the plan against the fact that the sim carries 52 of 172 agent-days.
- **Implements:** items 48, 49, 50, 51, 62 (as a crate-wide convention), 66, 67, 70; spec section 15 (Maths, Determinism).
- **Needs:** T0. Deliberately **not** T1 — the sim's state types are not proto types.
- **Acceptance:** `xtask ci`'s `determinism` step turns from *skipped* to green and `tests/golden/determinism/expected.hashes.txt` is committed; the CI matrix diffs the three chains (item 71's re-expression of the G4/G3′ `hash-identity` job); the 26 XXH3 reference vectors and the two pinned digests assert as tests; 50 save/restore round-trips hash-identically in fresh processes; fork equivalence 20/20 under `--features research`; **zero allocations per tick** asserted from day one (G3′ §9.17); a **source-text test that no clock and no `HashMap` reach the crate** — copying the test, not just relying on the lint, which is the one lesson G2 and G3′ both wrote down; a test that the rules table, the repath cap and every other performance knob sit **outside** hashed state (G3′'s calibration-constant lesson).
- **Contract PR:** yes — state hash, RNG stream enum, fixed-point types, snapshot format, tick loop, the `research` feature.
- **Agent-days:** 10. **Split seam if it goes long:** the public type surface + newtypes + RNG as PR 1, the encoder + hash + snapshot + tables as PR 2.
- **PLACEHOLDERs:** the CSR cell size (`tuning, owner, S2 exit — tied to unit density`, item 67's caveat); `DETERMINISM_TICKS` raised from 1 200 once the real sim carries a segment (owner, at T20); `rust-version` and `repository` in the workspace manifest (owner, once the GitHub org exists).

---

### T3 — `xtask`: harness part 2's *formats*, front-loaded

- **Crate owned:** `xtask` (+ `.github/workflows/ci.yml`, the `tests/golden/**` and `scenarios/**` layouts).
- **Builds:** the scenario file format (map seed + per-seat playbooks + segment list + assertions on events **and** hashes), documented and human-diffable; the golden-file conventions the existing `step_golden` already implements (`tests/golden/<area>/expected.*` compared against `<target>/golden/<area>/actual.*`), extended with a per-area note saying what a diff means; two new `xtask ci` steps that **skip with a named reason until their producer exists** — `scenario` (shells `gamectl scenario run`, filled by T15) and `screenshot` (renders under xvfb + lavapipe and compares to a committed PNG with G1's thresholds, filled by T16); the `godot --headless --path godot --import` pre-step every fresh checkout needs (G1 §10.12); and the `::notice::` annotation path, because GitHub hides job logs and step summaries from logged-out viewers (G1 §10.12).
- **Implements:** spec section 15 (Dev harness), section 17 (Harness first, in two parts); AGENTS.md §9 items 10–11; item 71.
- **Needs:** T0.
- **Acceptance:** `cargo xtask ci` lists both new steps and reports them *skipped, with a reason*, green; `--quick` and `--fix` still work; a **golden-format self-test** — a deliberately mismatched fixture produces a readable first-difference report; the screenshot step, pointed at a committed fixture PNG, passes on it and fails on a doctored one. The checkers are tested, not just the things they check.
- **Contract PR:** yes — `xtask`'s definition of `ci`, golden-file *formats*, `.github/workflows/**`. **This is what makes AGENTS.md §10 items 1–4 a command from the first merged PR** instead of from week 14, and it is why it is in wave 1 rather than the closing half-week.
- **Agent-days:** 6.
- **PLACEHOLDERs:** the scenario assertion vocabulary beyond "event fired" and "hash chain equals" (owner, at T15, when the runner meets real events); the screenshot thresholds carried from G1 (mean ≤ 0.0039/255, 0 pixels over 32 — owner re-ratifies at T16 when the real vista replaces the fixture).

---

### T4 — `crates/mesher`: greedy meshing, the light bake, the drain budget

- **Crate owned:** `crates/mesher`.
- **Builds:** the greedy mesher taking integer chunk data in and vertex buffers out, written against the real types with the `spike-end` worktree open, never copied; the `(material, light)` mask key; the flood-fill light bake **seeding every sky cell that has a taller horizontal neighbour** (G1 §10.12 — seeding only the lowest per column renders every overhang black and degenerates the mask key; this form is exactly equivalent and costs a handful per column); the dirty pad derived from `LIGHT_MAX` / `LIGHT_ATTEN` as `light reach + 1` rather than asserted; the K/B drain queue with item 54's order in full; Godot's **clockwise** front face under `CULL_BACK` with the quad walk emitted reversed; 16-bit indices; the integer/float wall line stated at the boundary function; and the headless CPU proxy that **links without gdext**, so CI can check geometry with no GPU.
- **Implements:** items 53 (the mesher half), 54, 55, 56; spec section 15 (Rendering, Art pipeline); G1 §10.12.
- **Needs:** T0. Independent of the sim — its input is a plain integer chunk view the crate owns (decision 17).
- **Acceptance:** `wall-guard` green; a **geometry golden** — per-chunk vertex and index digests for a fixed chunk set — committed and compared across Windows and Linux; quantised vertex colours compared byte-for-byte against what Godot will read; 16-bit index headroom asserted; per-chunk mesh CPU **p99** as a regression alarm against G1's 343 µs flat / 503 µs cratered, never a max (G2 §9.11: a max is not a property of the code); zero allocations per meshing; K and B read from the rules table, not from constants (item 54).
- **Contract PR:** no (the geometry golden's *format* is covered by T3).
- **Agent-days:** 8.
- **PLACEHOLDERs:** K = 4 / B = 512 KiB carry `tuning, owner — no measured frame-time reason separates K=4 from K=8 on the spike machine (item 54)`; the ageing term ships untested by measurement and is labelled insurance; `LIGHT_MAX` / `LIGHT_ATTEN` (Tuning, owner at S6's art polish).

---

### T5 — `crates/sim` part 2: the world, the chunk store and the seeded map generator

- **Crate owned:** `crates/sim`.
- **Builds:** the 32³ copy-on-write chunk store with the 64-layer cap, single-voxel edits, the crater primitive and per-chunk digests feeding T2's hash; the world tables (seats, beacons, units, structures, wrecks); and the seeded deterministic map generator on `Stream::Map` — about 384 × 384 × 64, spawn zones, one reachable heat vent and one starting scrap seam near each start, equal nearby ore, richer contested vents toward the centre, the **spawn-distance rule** (at least about 1.5 early Pushes apart, reported in both raider travel and commander travel), unoccupied spawn zones left as terrain only, and total supply kept inside the provisional per-map power ceiling. The map is written as a file.
- **Implements:** spec sections 3, 9 (maps, seams, vents), 15 (Map generator); items 55, 63, 65, 66, 67.
- **Needs:** T2.
- **Acceptance:** `mapgen_is_seed_deterministic` byte-identical on three OSes, with a committed per-seed map digest golden; `spawn_distance_holds_for_every_seed_in_the_set`; vent reachability and ore parity asserted across the **occupied** zones (the 3-way symmetry suite is S4's); `supply_is_within_the_power_ceiling`; the chunk digests are in the hash chain and the chain matches its golden; every new field added to the hash, the snapshot round-trip **and** the goldens in the same PR (AGENTS.md §4.8); the chain's movement explained in the PR.
- **Contract PR:** yes in part — extending the snapshot format and the hashed table set is determinism code.
- **Agent-days:** 7.
- **PLACEHOLDERs:** vents per map, ore yield per richness, map dimensions (Tuning, owner — decision 14); the 190 kW ceiling carries item 65's two PLACEHOLDERs inside it (decision 15).

---

### T6 — `crates/verifier`: QUICK, the diagnostic catalogue, `report_hash`

- **Crate owned:** `crates/verifier`.
- **Builds:** the pipeline at its full shape — decode → structure → resolve → semantics (QUICK) → estimate → lint (FULL) — with the QUICK stages complete and **FULL's estimate and lint stages present but empty**, so the API shape and the hash contract never change when S1 and S3 fill them (decision 11); the diagnostic catalogue as a data table across the families the spec names (E000x, E01xx, E02xx, E03xx, E04xx, E05xx, E0601/W06xx, W07xx, I…), each diagnostic carrying code, severity, a JSON Pointer, related paths, map references, a precise message, a plain-language beginner sentence and JSON Patch suggestions labelled by how safely they apply; the **never strip** rule — an out-of-vocabulary construct is rejected with a code and a pointer, on Load as well as on submit; and `report_hash` as a pure function of the five inputs (playbook bytes, snapshot, rules hash, verifier version, depth). All of it under the no-dry-runs rule: `pharmakos-sim` with `default-features = false`, nothing stepped or forked.
- **Implements:** spec section 11 in full, section 10 (limits and termination, Load never strips), item 46 (negative durations rejected here, with a pointer).
- **Needs:** T1, T2.
- **Acceptance:** `tests/golden/verifier/<case>/expected.report.json` for a case per implemented code, each pinning the full report and its `report_hash`; the **catalogue itself** committed as a golden, so a code cannot be renamed silently, and code numbers are append-only from the day they ship; a byte-identical-report test across the three CI OSes; `research-guard` green and a source-text assertion that the crate names no stepping API; the 10 000-playbook fuzz target exists and is wired into `nightly-scenarios.yml` behind its existing variable gate; QUICK p99 **measured and reported, not gated** — P1 QUICK is S1's depth task.
- **Contract PR:** no (golden formats only, and those were frozen in T3).
- **Agent-days:** 8.
- **PLACEHOLDERs:** the playbook size budget (owner, S3 exit, from P1 and the per-rule cost per decision tick — a provisional number is decision 19); reach memory N (owner, S2/S3); which of the ~35 codes the skeleton omits, listed by number in the catalogue golden (decision 18).

---

### T7 — `crates/sim` part 3: HPA\* and the travel estimator

- **Crate owned:** `crates/sim`.
- **Builds:** HPA\* at cluster 32 = one chunk footprint with a single abstract level; the integer cost model 10 / 14 / 4 with `MOVE_COST_PER_TICK = 3` and ceiling rounding, read from the rules table; the `(f, h, node_id)` hand-written binary min-heap with dense `u32`-indexed closed set and g-scores; the **union-find connectivity oracle from day one** (4.5 % of repaths return no route and the oracle answers them in 0.0001 ms); eager chunk-footprint repair with one CSR and connectivity rebuild per tick; the per-tick repath cap of 16 served round-robin by `(seat, beacon, unit)`; **"sealed in" as a designed state** — park and report to the seat; and `estimate(...) -> Option<Estimate { cost, ticks, legs }>` exactly as item 61 fixes it, abstract search only, fog priced `cost * 3 / 2` per abstract edge, no cache.
  Whether this lands as real HPA\* or as the frozen contract over a simpler search is **decision 8** — section 17 assigns HPA\* in the sim to S3's depth task, and item 71 says the path-identity comparison "joins it when pathing lands at S3".
- **Implements:** items 57, 58, 59, 60, 61, 62, 69; spec section 9 (locomotion), 11 (allowed estimates); G2 §9.11.
- **Needs:** T5.
- **Acceptance:** a committed `tests/golden/pathing/expected.path-hashes.txt` (G2's shape: route hashes over the pristine map plus crater repairs) compared across the three OSes; three repair tests — repair equals a from-scratch build, equals the conservative all-neighbours set, and holds at every cluster size; **the estimator's *signed* error is never negative at any percentile**, not merely |error| p90 ≤ 15 %, because item 61 makes never-optimistic part of the contract and the editor's rounding rests on it; a **repath-cap sustainability test** asserting the backlog does not grow over a full 8-minute segment at cap 16, with a `sustainable` boolean in the output (G3′ §9.17: a sweep table without backlog growth in it actively misleads); a bench reporting repath p99, estimate p99, **the endpoint-sweep share of an estimate query**, and **steps and components** rather than cell counts (G2 §9.11 — the endpoint-sweep number is the one S3 needs to decide whether attacking it is worth anything, and nobody else produces it); `no_path_is_answered_by_the_oracle` with a cost assertion.
- **Contract PR:** yes in part — `(f, h, node_id)` is already determinism contract (item 62), and the path-hash golden's format is one.
- **Agent-days:** 8.
- **PLACEHOLDERs:** `CLIMB_SURCHARGE = 4` ("no gameplay evidence behind it", item 59 — owner, S3); the repath cap 16 re-derived at S2's exit (item 69); the corner entrance per cluster recorded and deliberately not built (S3).

---

### T8 — `crates/plan-core`: canonical form, JSONC round-trip, patch, prose, arithmetic

- **Crate owned:** `crates/plan-core`.
- **Builds:** the canonical form that is authoritative for the file format; the JSONC reader and writer that round-trip **comments and formatting byte-for-byte**, which is the property the whole editor model rests on (spec section 13, "Exact round-trip"); JSON Patch with inverse-patch undo; `render_plan`'s deterministic template prose, reading the coming segment's length from the frozen snapshot rather than assuming it; interface-time arithmetic at the section-5 rates; the `$` and `kW` projection the Quartermaster's rules imply; `instantiate_template` over the local template folder the gateway *reads* and stores nothing from; and the pass-through to the sim's estimator, rendering a fogged leg as a labelled upper bound and a short route generously or in whole seconds (items 57, 61).
- **Implements:** spec sections 10, 11 (allowed estimates), 13; items 46, 57, 61.
- **Needs:** T1, T6. The estimator's *signature* is frozen by item 61, so the call site is written against the contract and wired when T7 merges — a frozen contract, not a cross-agent handshake.
- **Acceptance:** `tests/golden/plan-core/expand_east/expected.jsonc` reproduced byte-for-byte from `examples/playbooks/expand_east.jsonc` through load → canonicalise → save, comments in awkward places intact; a canonical-form golden pinning the writer; `expected.prose.txt` for `render_plan`; a property test that patch-then-inverse-patch is the identity on every fixture; `research-guard` plus a source-text test that the crate names no stepping API; the writer proved to emit bare-number durations exactly as the example writes them.
- **Contract PR:** no, but the canonical form *is* a golden format — flag it in the PR.
- **Agent-days:** 10. **Split seam:** the JSONC codec and canonical form as PR 1; patch, prose and the arithmetic as PR 2.
- **PLACEHOLDERs:** the English string table's home and wording (owner, S6); the size meter's budget number (decision 19).

---

### T9 — `crates/gateway` part 1: transport, tokens, scopes, fog, feed, cache

- **Crate owned:** `crates/gateway`.
- **Builds:** the security surface, once, to full quality, **before any method hangs off it** — this sequencing is the single change that stops a rewrite when v1.1 publishes the gateway. JSON-RPC 2.0 over a WebSocket bound to `127.0.0.1` and `::1` only, with `Host` and `Origin` checks on the upgrade and no flag that widens it; 256-bit per-seat tokens tied to a match and a seat, revocable from the lobby; the scope set (`observe`, `plan`, `plan.submit`, `docs`, `spectate.nofog`, `admin`) enforced per method from T1's annotation, with the invariants asserted in code — a seat token can never hold `spectate.nofog`, `admin` can never read another seat's playbooks, drafts or knowledge; the **fog filter as a per-match server-side policy** (fogged by default, no-fog in casual, unlocked on elimination and at match end) so no token is reissued mid-match; rate limits and the audit log **as part of the feature, not a later hardening task**; the event bus and `get_segment_feed`'s 60 s digest cadence; the frozen planning snapshot with opaque snapshot-tied cursors; the private match cache as plain local state, documented as enforcement and not cryptography; the closed error set and the `_status` footer; and the read-method detail budgets in the fixed salience order, kept because they are the shape v1.1 needs.
- **Implements:** spec section 12 (Security, Budgets, Output, Errors, During play), section 3 (fog policy, secrecy, live standings show own score and rank only), section 15 (Hosting); AGENTS.md §7.
- **Needs:** T1, T2.
- **Acceptance:** negative tests as first-class citizens, each named after the rule it enforces — `binding_is_localhost_only`, `an_origin_mismatch_is_refused`, `a_seat_token_can_never_hold_spectate_nofog`, `admin_cannot_read_another_seats_draft`, `a_fogged_seat_never_sees_an_unseen_voxel`; a fog-filter golden replaying one tick's event set through each seat's filter; rate-limit and audit-log tests, the limiter counting deterministically and never reading a clock inside the sim; a test that nothing a playbook can contain reaches the filesystem or a socket.
- **Contract PR:** no by path — but review it as one. The scope table lives in `gp.api.v1` (T1) and this is the security surface the whole roadmap inherits. **If a method needs a `gp.api.v1` change, the change goes back to T1 as a proto PR rather than being made here.**
- **Agent-days:** 8.
- **PLACEHOLDERs:** rate-limit numbers and token lifetime (owner, hardening); the private match cache's location and on-disk layout (decision 20); the detail budgets carry `no v1 client is a model` (spec section 12).

---

### T10 — `crates/sim` part 4: the runner, the phases and the event bus

- **Crate owned:** `crates/sim`.
- **Builds:** Lull / Push / recap as phases with their fixed order; the segment ladder and the per-round length list, with the coming segment's length carried **in the frozen snapshot** rather than read from a constant; the frozen segment-end snapshot itself; the match-end one-tick rule and the round limit; elimination (units disband, structures become neutral ruins); commander respawn as per-Push state that always completes by segment end; and the event bus the gateway's feed and the watch rig read.
- **Implements:** spec section 3 (match structure, segment ladder, match end, elimination), section 4 (commander death), section 15; item 68.
- **Needs:** T7.
- **Acceptance:** a golden hash chain over a full segment; `the_coming_segments_length_comes_from_the_snapshot_not_a_constant`; `a_pending_respawn_always_completes_by_segment_end`; the match-end rule asserted at the tick, not at the segment boundary; the chain's movement explained in the PR.
- **Contract PR:** yes in part — the tick loop and the snapshot format.
- **Agent-days:** 5.
- **PLACEHOLDERs:** respawn delay curve (Tuning, decision 14).

---

### T11 — `crates/sim` part 5: the playbook interpreter at the v1 vocabulary

- **Crate owned:** `crates/sim`.
- **Builds:** decisions every 250 ms of game time; one rule body at a time with no pre-emption; the fixed 20 % reflex as the only interrupt — not disableable, no `reflex_hp_pct`, aborting a visit, walking to the safest own beacon, re-firing only after new damage; route steps `move`, `interface`, `place_beacon`, `wait_until`, `hold` and `broadcast` (the last rejected by the verifier with no Mast, never a silent no-op), each with `skip_if`, `timeout_ms`, `on_fail` (skip / abort route / jump forward only), `label` and `note`; handlers with `all`/`any`/`not` condition trees at depth ≤ 4 and ≤ 24 nodes, integer comparisons only, explicit `resume`, `cooldown_ms ≥ 5 s` and `max_fires` 1–8; late-bound selectors resolved at step start and pinned for that step; the required `on_death` and `fallback` blocks (hold / shadow / patrol); atomic interface rows committing at the end of their own durations at the section-5 rates, the handshake paid again on a resumed visit, damage not interrupting; and locomotion under the mandate's orders — walking, one-voxel climbs, blocked by walls, craters and trenches.
- **Implements:** spec sections 4, 5, 10 (execution, steps, conditions, limits and termination); items 24, 30, 40, 46, 60.
- **Needs:** T10, T1, T6.
- **Acceptance:** `tests/golden/interpreter/` — one committed playbook per vocabulary construct, each with a pinned transcript of decisions, step transitions and commit points; `every_playbook_halts` as a property test over the fuzz corpus; `a_jump_only_goes_forward` and `every_wait_has_a_timeout`; `the_reflex_interrupts_a_visit_and_re_fires_only_after_new_damage`; `a_resumed_visit_pays_the_handshake_again`; a scenario file asserting on both events and hashes for a full 3-minute segment driven by `expand_east.jsonc`.
- **Contract PR:** no beyond the hash-chain explanation.
- **Agent-days:** 8.
- **PLACEHOLDERs:** interface times quoted from spec section 5 as rules-table data (Tuning); the condition families the skeleton's templates do not use stay stubs naming their stage.

---

### T12 — `crates/client-gdext` + `godot/`: the thin bridge, on a fresh checkout

- **Crate/unit owned:** `crates/client-gdext` and the `godot/` project directory, treated as **one ownership unit** under AGENTS.md §6 (decision 1), held by one author for the whole stage across T12, T16 and T19.
- **Builds:** the Godot project (`project.godot`, `vsync_mode=0`, `delta_smoothing=false`, `Engine.physics_jitter_fix = 0`, `forward_plus`, Single-Safe threading), the `.gdextension`, and `godot/bin/` as the staging target; the marshalling layer and nothing else — gateway calls in, JSON results out; mesher buffers in, `RenderingServer` RIDs out; **no rules, no time arithmetic, no validation, no gameplay decision** (AGENTS.md §3 rule 4); path B with path A alive as a build-time switch and the four guards item 53 names as the price of path B — no `Gd<Material>` dropped after `get_rid()`, the surface-format probe, the **index-array equality check** before an in-place region update (29 of 2 570 updates in G1's headline run had changed indices), and colour quantisation matching Godot byte for byte; a **self-counting caught-panic guard** at every `#[func]` boundary, because gdext's catch is silent and turns a panic into a healthy-looking result; and `cargo xtask stage-client` — build the cdylib → copy into `godot/bin/` → `godot --headless --path godot --import`, with a **byte-scan of the staged library for `gdext_rust_init`** before declaring success, because the two-target-directories trap reads like an ABI failure and is not one.
- **Implements:** items 52, 53, 56; spec section 15 (Rendering, Languages, Platforms); G1 §10.12.
- **Needs:** T1, T4, T9. The bridge is unit-tested against committed `gp.api.v1` fixture responses, which is marshalling and needs no live server; it meets the real gateway at T16.
- **Acceptance:** CI builds the cdylib on Windows and Linux, runs `--import` first (without it a fresh checkout instantiates the extension's classes as placeholders — G1 lost four runs to this), then asserts the caught-panic count is **0**; paths A and B produce identical geometry digests on the same chunk set; a source-level test that the bridge module performs no arithmetic on `$`, `kW` or durations; `wall-guard` unchanged; **no dependency edge on `pharmakos-sim`** — the crate map grants this crate gdext, proto and the mesher's public surface, and nothing else.
- **Contract PR:** no. The `.gdextension` shape, the project layout and the version pins are decisions 1 and 2, settled before the task starts.
- **Agent-days:** 9.
- **PLACEHOLDERs:** the surface format's vertex layout (`owner/S6 art pass — a heavier layout is what B = 512 KiB's margin is for`); the `unsafe_code` scoped allow gdext needs, raised in this PR rather than edited into the contract `[workspace.lints]` table (decision 12).

---

### T13 — `crates/gateway` part 2: the method slice

- **Crate owned:** `crates/gateway`, same author as T9.
- **Builds:** every method the skeleton's clients call, on the surface T9 froze — `get_status`, `wait_for`, `save_notes` (4 000-character notebook, under the `plan` scope, returned at the top of `get_briefing`), `get_briefing`, `get_recap`, `list_beacons`, `get_beacon`, `get_map_summary`, `get_economy_forecast`, `estimate_route`, `get_schema` (generated from the schema so it cannot drift), `list_templates` / `instantiate_template`, `verify_plan{depth}`, `render_plan`, `patch_plan`, `save_draft` / `list_drafts`, `get_safe_plan`, `submit_plan` (always FULL; the latest verified submission replaces the previous one, any number of times), `set_ready`, `get_segment_feed` — plus the match host that drives the runner, and draft continuity (last round's playbook pre-loaded and re-verified against the new snapshot).
- **Implements:** spec section 12 (all method groups), section 13 (draft continuity), section 14 (the safe playbook is fetched here).
- **Needs:** T9, T8, T10, T6.
- **Acceptance:** the spec's **14-call walkthrough** replayed end to end as a scenario assertion, ending with `submit_plan`'s `report_hash` equal to the FULL pre-check's; `get_schema` output committed as a golden and compared by the schema step; secrecy tests (a second seat's token sees neither playbook nor draft nor notebook nor replay); a test that an invalid playbook returns a full report with `qualifies:false` and is **not** a method error.
- **Contract PR:** no — unless a method needs a `gp.api.v1` change, which goes back to T1.
- **Agent-days:** 9.
- **PLACEHOLDERs:** the digest payload's exact contents; the template folder's location on each platform (owner, with packaging at T21).

---

### T14 — `crates/sim` part 6: economy, the minimal Build mandate, Survey-lite, programs

- **Crate owned:** `crates/sim`.
- **Builds:** the **Quartermaster trait** and its single-treasury stub — hold fabricator orders first, then the brownout order; fill spend requests by the urgency ladder, round-robin within a band; the per-beacon low/normal/high priority knob, kW-only; the **Mandate trait** and a minimal Build mandate — the Generator blueprint, build targets, the two starting build drones, fabrication only while there is outstanding work; **Survey-lite** — probe areas, scout count, sightings carrying an age that accrues on match game time, refreshed closest-to-leaving-the-window first; the built-in **programs** for move, build, mine and scout; `$` with mining and salvage credited on delivery, "paid means yours" (charged at commit, full value from that instant), value following condition (build cost × current HP), and BMI settled at each recap by rank on held value among living seats; `kW` with the core's deep-bore surplus, one Generator per vent at the vent's richness, draw per fielded item, the brownout order (lowest priority, then furthest from the core, core last, autocannon never shed), dormancy powering down everything homed to a beacon, and the revive margin; and the per-seat kill-credit counters (at most three per asset) with their largest-remainder apportionment, **hashed from the day they exist** even though combat is S2 — a field that affects behaviour and is not hashed is a latent desync.
- **Implements:** spec sections 5, 6 (Build, Survey, Common), 7, 9; items 10, 17, 18, 19, 20, 22, 63, 65.
- **Needs:** T11.
- **Acceptance:** `tests/golden/economy/expected.settlement.txt` for a fixed three-round match, line by line; `value_follows_condition_at_the_audit_and_at_the_recycle_refund`; `paid_means_yours_survives_a_mid_build_death`; `a_dormant_beacon_powers_down_everything_homed_to_it`; `the_brownout_order_is_priority_then_distance_then_the_core_last` and the autocannon never shed; `no_beacon_starves_another_within_a_band`; the hash chain extended with the treasury, power and kill-credit tables — all three in the hash, the snapshot round-trip **and** the goldens in the same PR; a tick bench reported against item 64's five-number shape, **as a number, not a gate** (AGENTS.md §9 item 11).
- **Contract PR:** yes in part — hashed state extends.
- **Agent-days:** 10. **Split seam:** `$` and the Quartermaster as PR 1; `kW`, the grid and brownout as PR 2.
- **PLACEHOLDERs:** every `$` and `kW` number — BMI, starting `$`, structure and unit costs, core surplus, Generator output per richness, revive margin, backlog threshold, sphere radius, placement range, commander speed, ruin salvage value (decision 14, all as rules-table rows, never as constants in code).

---

### T15 — `crates/gamectl`: the CLI and `scenario run` *(harness part 2's runner)*

- **Crate owned:** `crates/gamectl`, plus the one `xtask` PR that fills T3's `scenario` step.
- **Builds:** `verify`, `schema`, `docs` (generated from the schema, drift-checked), `seat doctor`, and **`scenario run`** — hosting a headless match from a scenario file and asserting on events *and* hashes, which is why the gateway is on this crate's dependency list. **No `connect` in v1.**
- **Implements:** spec sections 12 (gamectl), 15 (Dev harness, "hash parity with the client path"); AGENTS.md §9 item 10, §10 item 4.
- **Needs:** T3 (the format), T13, T8, T6.
- **Acceptance:** `xtask ci`'s `scenario` step turns from *skipped* to green over the committed `scenarios/**`; a deliberately doctored assertion produces a readable diff and a non-zero exit; **the headless runner's hash chain equals the client path's for the same seed**; `gamectl verify examples/playbooks/expand_east.jsonc` reproduces the verifier golden's `report_hash`; exit codes documented.
- **Contract PR:** yes for the `xtask` half (one xtask PR open at a time).
- **Agent-days:** 8.
- **PLACEHOLDERs:** the Rusher / Turtle / Hunter playbooks and their seed set are **S2** and not written here; `NIGHTLY_SCENARIOS_ENABLED` stays unset.

---

### T16 — `godot/` + `crates/client-gdext`: the vista and the watch rig

- **Unit owned:** `godot/` + `crates/client-gdext`, same author as T12.
- **Builds:** the one real vista — terrain through the mesher, voxel destruction remeshing inside the K/B budget, emissive plus baked flood-fill light under the Pall, placeholder palette models; and the watch rig section 17's row names: **one** camera rig with the own-fog view, free-look and a follow-commander toggle, the live event list from `get_segment_feed`, 2–4× speed, free and instant skip to segment end, and the full map unlocking on elimination or at match end **through the gateway's server-side fog policy, not a client-side flag**. The Lull timer lives here, on the walled side, because it is a UI and host concern the sim never reads.
- **Implements:** spec sections 3 (During play), 13, 15 (Art pipeline); items 2, 53, 54.
- **Needs:** T12, T13, T10.
- **Acceptance:** the committed 1280 × 720 vista golden passes `xtask ci`'s `screenshot` step under xvfb + lavapipe at G1's thresholds, with the shot taken on a frame **outside any measured series** (a screenshot costs a frame — G1 measured 67–91 ms on the shot frame); `speed_2x_and_4x_produce_the_same_hash_chain_as_1x` and `skip_to_segment_end_produces_the_same_terminal_hash_as_playing_it_out` — two cheap tests that prove no wall clock and no presentation concern leaked into the sim; the event list's contents compared against `get_segment_feed`'s JSON as a golden; `the_full_map_unlock_comes_from_a_server_side_policy_change`; caught-panic count 0.
- **Contract PR:** no.
- **Agent-days:** 10. **Split seam:** the vista as PR 1; the watch rig as PR 2.
- **PLACEHOLDERs:** camera speeds and easing (walled floats, Tuning); every art and audio asset placeholder with its SPDX line and credits-screen entry pending (owner, S6); the screenshot tolerance re-ratified here (decision 22).

---

### T17 — `crates/sim` part 7: save, resume and the private deterministic replay

- **Crate owned:** `crates/sim`.
- **Builds:** save at Lull boundaries from the frozen segment-end snapshot, stamped with the rules hash and verifier version and refusing to load on a mismatch; and the **private deterministic replay** (seed + playbooks + log) that the scenario runner, the recap and bug reports read and that is never exposed in v1. The **shareable recording format is S7 and is not built here.**
- **Implements:** spec section 3 (Save and resume), section 15 (Replays, Saves); item 49. Subject to decision 13.
- **Needs:** T14.
- **Acceptance:** a save/restore at every Lull boundary of a three-round match round-trips hash-identically; a replay of a committed match reproduces its hash chain exactly; a rules-hash mismatch refuses to load, asserted; the replay is written into the private match cache and a test asserts no other seat's token can read it.
- **Contract PR:** yes — the snapshot and replay formats.
- **Agent-days:** 4.
- **PLACEHOLDERs:** none new.

---

### T18 — `crates/operator`: Easy, and the safe playbook

- **Crate owned:** `crates/operator`.
- **Builds:** the built-in operator as an **ordinary gateway client** — same snapshot, same verifier, same submit path, no privileged reads, and it must not name a sim-internal type; deterministic, seeded from (match, seat, round), budgeted in evaluation units and never in wall time; integer situation features (threat, power headroom, vent sites, expansion sites, commander risk); template parameters filled with top-k at Easy's row (30 candidates / top-3, 70 % route fill, flee rule only, fixed targets only, **no lookahead at any level** — barred by "no dry runs"); visit goals composed by greedy insertion by utility per second until the route fills its share of the segment whose length comes from the snapshot; utility scored defence + economy + expansion + capability + attack − risk on the Balanced weighting; the standard rules, then verify-and-repair up to four times. Plus the **safe playbook**: leave configurations untouched, move to the safest beacon; if power is short and up to two at-risk beacons are within 60 s travel, raise their Quartermaster priority nearest-first (never recycle, switch mandate or place); otherwise shadow the safest beacon with the flee rule. It always qualifies, and the editor renders it. Ships the three skeleton templates as JSONC data files.
- **Implements:** spec section 14 (the Easy column and the safe playbook); AGENTS.md §3 rule 3.
- **Needs:** T13, T8, T14.
- **Acceptance:** `the_operator_is_deterministic_for_a_given_match_seat_round`, byte-identical on three OSes, as a golden playbook per seed; a 100 % verifier pass rate over the snapshot corpus; `the_safe_playbook_always_qualifies` on every fixture snapshot including a power-short one; `the_operator_names_no_sim_internal_type` (source-text test alongside the dependency walk); a test that the crate reads no clock. **P2's timing numbers are S5's and are not asserted here** — the safe playbook's latency is measured as a baseline only.
- **Contract PR:** no.
- **Agent-days:** 8.
- **PLACEHOLDERs:** the Balanced utility weights (Tuning, owner at S5); target spread is S5 and deliberately absent; which three templates ship is decision 10.

---

### T19 — `godot/` + `crates/client-gdext`: the editor wizard, the notes box, submit

- **Unit owned:** `godot/` + `crates/client-gdext`, same author as T12 and T16.
- **Builds:** the parameter wizard over three templates, each page pre-filled with the operator's suggestion and a "why" note; the map route surface (click a beacon for Visit & change / Go here / Recycle; click ground for Go here / Place beacon with a live legality ghost; Alt-click turns a fixed target into a selector); routes as polylines with travel estimates, **fogged legs dashed and labelled as bounds, never as ETAs**; the rule list as sentences with parameter chips; QUICK on every edit and FULL after 600 ms idle, rows showing icons and plain sentences with Fix buttons applying the verifier's machine-applicable patches; Load rejecting an out-of-vocabulary construct with the code and pointer rather than stripping it; the notes box over `save_notes`; draft continuity; submit and `set_ready`. **The editor runs no validation and no time maths of its own** — it asks the gateway.
- **Implements:** spec section 13 (the skeleton's slice), section 12 (the editor is one more client); items 57, 61.
- **Needs:** T13, T16, T18 (its `instantiate_template` response shape is frozen in T1, so the wizard is built against a committed fixture response until T18 merges).
- **Acceptance:** a headless-driven run per template — wizard → parameters → patch → verify → submit — asserting that the editor's bytes equal `plan-core`'s canonical bytes and that `gamectl verify` accepts the file; a byte-identical round-trip of a file opened and saved in the editor; `the_editor_makes_no_time_arithmetic_of_its_own`, a source-text test over `godot/**.gd` for arithmetic on `$`, `kW` or durations; screenshot goldens for the wizard's first page and the diagnostic rows; accessible names generated from the rendered sentences on all three templates; a written note answering spec section 19's open item — **whether GraphEdit is still experimental in Godot 4.7** — reported, not depended on. **P6's ≤ 2 min / ≤ 5 min targets are S3's gate and are not asserted here.**
- **Contract PR:** no.
- **Agent-days:** 10. **Split seam:** the wizard and validation rows as PR 1; the map route surface as PR 2.
- **PLACEHOLDERs:** the string table's file and format (owner, S6); accessibility polish beyond generated names deferred to S6; the segment clock and fits pill are S3/S6 and are not built.

---

### T20 — `xtask` + `.github/workflows`: the closing half-week

- **Crate owned:** `xtask` and `.github/workflows/**` (one xtask PR open at a time).
- **Builds:** the cross-OS jobs item 71 promised would replace the deleted spike workflows — the hash chains, the path-hash file and the geometry golden compared across Windows, Linux and macOS in one job; `scenario` and `screenshot` promoted from skipping to required; the `schema` regenerate-and-compare step (AGENTS.md §9 item 8); the demo scenario set; and the perf **alarms** (mesher p99, tick-minus-pathing, allocations per tick) published as `::notice::` annotations, **not gates** — AGENTS.md §9 item 11 puts budgets with the gates that set them, which are S1's and S2's.
  Carrying G3′'s companion lesson: **a number that cannot vary across platforms must not be compared across them.** The three-OS job compares only the columns that can vary — hashes, path hashes, geometry digests — and the perf alarms are per-runner, because a green matrix on a number that reproduces by construction is false reassurance.
- **Implements:** items 52 (the `frame-time` job stays `if: false` until a GPU runner exists), 71; AGENTS.md §9 items 6, 7, 8, 10, 11; §10 items 1–4.
- **Needs:** T15, T16, T19.
- **Acceptance:** one `cargo xtask ci` invocation covers AGENTS.md §10 items 1–4 with nothing skipped; the matrix is green on three OSes; a deliberate one-bit change to any golden turns it red with a readable diff.
- **Contract PR:** yes.
- **Agent-days:** 5.
- **PLACEHOLDERs:** the perf alarm thresholds (decision 23).

---

### T21 — Packaging: the unsigned zips *(section 17's ~1 wk)*

- **Unit owned:** `godot/export_presets.cfg`, `cargo xtask package`, `.github/workflows/release.yml`.
- **Builds:** Godot export presets for Windows and Linux with the export templates pinned by version; the staged cdylib beside the executable; `gamectl` shipped alongside; the template and sample folders where T13 expects them; `LICENSES/` and the credits screen's CC-BY attributions (a shipping requirement, not a nicety); the two-line note on opening unsigned builds; and a release workflow cutting both zips from green CI on a tag with a changelog. **No signing, no notarisation, no macOS release** — that is hardening's ~3 wk (item 8).
- **Implements:** item 8; spec section 15 (Platforms & packaging), section 17.
- **Needs:** T16, T19.
- **Acceptance:** a zip built by CI launches on a clean Windows runner and a clean Linux runner and reaches the lobby (with `--import` run first); `gamectl seat doctor` passes from inside the extracted zip; `reuse` green over the packaged tree; the zip's manifest committed as a golden so its contents cannot drift.
- **Contract PR:** yes — `.github/workflows/**`.
- **Agent-days:** 5.
- **PLACEHOLDERs:** zip contents, version string and tag scheme (decision 24); the product name in the export preset and the repository URL (owner, once the org exists).

---

### T22 — Integration, goldens and the stage demo

- **Owned:** whatever crate each fix lands in, one at a time.
- **Builds:** nothing new. Closes AGENTS.md §10 as a checklist in one PR: `cargo xtask ci` green on all three OSes with the chains agreeing; every golden committed and every movement explained in the PR that moved it; the golden playbooks round-tripping, verifying and rendering identically; `gamectl scenario run` passing the stage's scenario files on events **and** hashes; docs updated; the collected list of every `PLACEHOLDER` the stage left, grouped by who resolves it and when; and the demo run sheet for §1.1.
- **Needs:** everything.
- **Acceptance:** the definition of done, item by item, ticked in the PR.
- **Contract PR:** no, except where a fix lands in one — then it is its own PR.
- **Agent-days:** 6.

---

## 4. The schedule

Three slots, never more (AGENTS.md §6), each agent in its own worktree, never two agents on one
crate. `crates/sim` has exactly one author for the whole stage; so do `crates/gateway` (T9 → T13)
and the `godot/` + `client-gdext` unit (T12 → T16 → T19), so the surfaces the rest of the project
inherits have a single author rather than three.

Days are working days, five to a week. A task's start is the day its last input **merged**.

| Wave | Weeks | Slot 1 — sim (one author) | Slot 2 — schema, gateway, operator | Slot 3 — harness, mesher, plan core, client | What the owner can see at the end |
|---|---|---|---|---|---|
| W0 | 0.0 – 0.4 | — | — | **T0** harness PR (2 d) | `xtask ci` green on a workspace with the mesher crate; `spikes/` retired at `spike-end` |
| W1 | 0.0 – 2.0 | **T2** determinism core (10 d) | **T1** proto envelope (8 d) | **T3** harness-2 formats (6 d), then **T4** mesher starts (d6) | A committed cross-OS hash chain; `buf breaking` green on the filled envelope; `xtask ci` naming the scenario and screenshot steps as *skipped, with a reason* |
| W2 | 2.0 – 3.4 | **T5** world + map generator (7 d, d10–17) | **T6** verifier QUICK (8 d, d10–18) | **T4** mesher finishes (d14) | A seeded map as a per-seed digest and as a PNG from the headless mesher proxy; verifier reports with `report_hash` on fixture playbooks |
| W3 | 3.4 – 5.0 | **T7** HPA\* + estimator (8 d, d17–25) | **T9** gateway surface (8 d, d18–26) | **T8** plan-core (10 d, d18–28) | The path-hash file agreeing on three OSes; the estimator's signed-error and sustainability reports; a hand-written JSONC playbook round-tripping and rendering as English prose |
| W4 | 5.0 – 7.4 | **T10** runner (5 d, d25–30), then **T11** interpreter (8 d, d30–38) | **T13** gateway method slice (9 d, d30–39) | **T12** client bridge (9 d, d28–37) | The spec's 14-call walkthrough passing against a live gateway; the extension loading in Godot on a fresh checkout with zero caught panics |
| W5 | 7.4 – 9.8 | **T14** economy, Build, Survey-lite, programs (10 d, d38–48) | **T15** gamectl + `scenario run` (8 d, d39–47) | **T16** vista + watch rig (10 d, d39–49) | **Flying over the generated map, craters remeshing**; the watch rig playing a segment at 2–4× and skipping to its end; `gamectl scenario run` as a green `xtask ci` step |
| W6 | 9.8 – 11.8 | **T17** save/resume + private replay (4 d, d48–52) | **T18** operator Easy + safe playbook (8 d, d48–56) | **T19** editor wizard (10 d, d49–59) | **A sealed playbook walking the commander**: a beacon placed, a Generator built, the treasury and kW meter moving, BMI settled at the recap — and a two-seat match against Easy |
| W7 | 11.8 – 12.8 | **T21** packaging (5 d, d59–64) | — | **T20** xtask closing half-week (5 d, d59–64) | A zip that launches on a clean Windows and a clean Linux machine; one `xtask ci` covering §10 items 1–4 with nothing skipped |
| W8 | 12.8 – 14.2 | — | — | **T22** integration, goldens, demo (6 d, d64–71) | The stage demo of §1.1 |
| — | **14.2 – 15.5** | **float: 1.3 weeks** | | | |

**Totals.** 23 tasks, **172 agent-days** over 71 working days of schedule. At three slots that is 213
slot-days, so utilisation is about 80 % — the 20 % is where a contract PR waits for review, and it is
deliberate. The critical path is **T0 → T2 → T5 → T7 → T10 → T11 → T14 → T18 → T19 → T20 → T22**,
71 days end to end; the sim alone carries 52 agent-days across six of the eight waves.

**Two consequences, stated plainly.**

1. Because one crate takes one agent and item 70 keeps the maths, the RNG, the hash, HPA\* and the
   map generator inside `pharmakos-sim`, a slip in the sim costs weeks one for one. Every wave's
   other two tasks are deliberately chosen to be *unblocked by the sim's current wave* — the proto
   envelope, the harness formats, the mesher, the verifier's structure work and packaging all need
   nothing from it — so a sim slip costs weeks but stalls nothing else. The two mitigations are
   built in rather than hoped for: **T2 publishes the sim's public snapshot, knowledge and rules
   types on day one** under `default-features = false`, and **T1 is scheduled first** because every
   other lane compiles against it.
2. The 1.3 weeks of float is not padding. Its named consumers are: the review latency of the seven
   owner-merged contract PRs (T1, T2, T3, T5, T7, T10, T14, T15, T17, T20, T21 — of which the seven
   heaviest are the real queue); the gdext and Godot-CI integration that bit G1 four separate ways;
   and the near-certainty that at least one of the three 10-day tasks splits at its named seam.

**Demo cadence.** Something the owner can look at lands at the end of every wave, and the first
thing that looks like a game — the vista — is at about week 9.8 rather than at the end. Waves W1–W3
produce artefacts to *read* (hash chains, digests, reports, prose) rather than to watch; that is the
honest cost of the risk ordering, and W4's gateway walkthrough plus W5's vista are where it converts.

---

## 5. Harness part 2

Spec section 15 and section 17 both put harness part 2 in "a named half-week at the skeleton's end".
This plan **splits it into three**, and the split is owner decision 4 because it touches `xtask`'s
definition of `ci`.

- **The formats go first (T3, wave 1, 6 d).** The scenario file format, the golden-file conventions
  and the headless-screenshot comparison land before any producer exists, as `xtask ci` steps that
  skip with a named reason — which is idiomatic to the existing `xtask`, since `step_golden` already
  skips when `tests/golden` is empty and `step_determinism` already skips until a `determinism`
  binary exists and self-activates the moment one does. Every later task then delivers *into* a
  format instead of inventing one, and no golden's shape is renegotiated under deadline in week 14.
- **The runner arrives where it is needed (T15, wave 5, 8 d).** `gamectl scenario run` lands the
  moment the gateway, verifier and interpreter can host a headless match — one wave before the vista
  and the editor need scenario assertions, not after. Built last, it would certify nothing that was
  written before it.
- **The closing half-week survives (T20, wave 7, 5 d).** The three-OS comparisons that replace the
  deleted spike workflows (item 71), promoting `scenario` and `screenshot` from skipping to required,
  the schema drift check, and the demo scenario set.

**The golden formats this stage freezes,** all under `tests/golden/`, every one human-diffable
(AGENTS.md §9 item 7): `determinism/expected.hashes.txt` (already wired at `xtask/src/main.rs`),
`pathing/expected.path-hashes.txt`, `proto/`, `plan-core/` (canonical form, JSONC round-trip,
`render_plan` prose), `verifier/` (reports, `report_hash`, the catalogue), `interpreter/`,
`economy/`, `mapgen/`, `mesher/` (geometry digests), `schema/` and `docs/`, `scenarios/*` with their
event logs, and the vista PNGs. Later stages add *files*, never formats.

**Deliberately not in harness part 2:** the three adversarial scenarios and their alarm bands (S2 —
`nightly-scenarios.yml` stays gated off); the 10 000-playbook fuzzer's CI leg (the target exists at
T6; the nightly run starts when the generator does); the perf **budget** steps (AGENTS.md §9 item 11
— they arrive with the gates that set them, and a number guessed now is a number to unpick later);
and G1's `frame-time` job, which stays `if: false` until a GPU runner exists (item 52).

---

## 6. Owner decisions before the stage starts

Recommendation first, marked **(Recommended)**, with the downsides of every option. Each is a
separate item, answerable one at a time. Decisions 1–12 gate the wave shown.

---

**1. Where the Godot project lives, and whether `godot/` is an ownership unit.** *Gates T12 (wave 4);
answer before wave 3.* The spec is silent on both.

- **(Recommended) `godot/` at the repository root** — `godot/project.godot`, `godot/scenes/`,
  `godot/scripts/` (GDScript views and editor UI only), `godot/bin/` as the staging target,
  `godot/.godot/` git-ignored with `godot --headless --path godot --import` run first in every CI job
  and every packaging step; and **`godot/` declared one ownable unit paired with `crates/client-gdext`**
  under AGENTS.md §6, so exactly one agent holds both at a time. *Downside:* it serialises the vista
  agent against the editor agent, which the schedule above pays for by giving one author T12, T16 and
  T19 in sequence.
- *Alternative — `godot/` inside `crates/client-gdext/`:* keeps the pairing implicit. *Downside:* a
  `res://` path cannot escape the project folder, the staged library then sits under a cargo-managed
  directory, and the crate map's "the client is thin" rule gets harder to read.
- *Alternative — `godot/` as a free-for-all any agent may touch:* faster in the short term.
  *Downside:* two agents editing GDScript views at once is exactly the collision §6 exists to prevent,
  and GDScript has no compiler to catch the merge.

---

**2. The gdext and Godot version pins, and the `.gdextension` shape.** *Gates T12; answer before wave 3.*
`Cargo.toml` carries `# godot = "0.3"` as an explicit PLACEHOLDER; spike G1 measured **gdext 0.5.5
against Godot 4.7.2**.

- **(Recommended) pin `godot = "=0.5.5"` and Godot 4.7.2 exactly**, in CI and in the export presets,
  with a single `godot/pharmakos.gdextension` (`entry_symbol = "gdext_rust_init"`,
  `compatibility_minimum = 4.7`, `reloadable = false`, explicit per-target library paths for
  `windows.x86_64` and `linux.x86_64`, debug and release), and treat a bump as determinism-adjacent
  because it re-runs the geometry golden. *Downside:* pinning delays upstream fixes, and a Godot
  security or platform fix would need a deliberate bump PR.
- *Alternative — a caret range:* picks up fixes automatically. *Downside:* every G1 number was taken
  on that exact pair, and a silent ABI or renderer change would show up as a moved geometry golden
  with no commit to blame.

---

**3. The canonical JSON codec — and it is a dependency decision either way.** *Gates T1 (wave 1).*
`proto/buf.gen.yaml` names `protoc-gen-prost-serde` as an explicit PLACEHOLDER. **Neither
`prost-serde` nor `pbjson` is on AGENTS.md §3's approved dependency list**, so adding one is itself
an owner decision under §3 rule 5.

- **(Recommended) a hand-written canonical JSON codec in `crates/proto`**, driven by the descriptor
  set, with the JSONC comment-and-formatting layer in `plan-core` on top of it. The requirement is a
  byte-exact round-trip that preserves comments, formatting and field order (spec section 13), which
  is not what a general-purpose serde mapping promises — serde can round-trip values; it cannot
  round-trip comments. *Downside:* we own a codec that must track the proto3 JSON mapping by hand;
  the round-trip goldens in T1 and T8 are what hold it honest, and it is a few hundred lines that
  nobody else maintains.
- *Alternative — add `protoc-gen-prost-serde`:* less code to own for the `gp.api.v1` wire messages,
  where no human edits the bytes. *Downside:* an unapproved dependency (§3 rule 5), two JSON paths in
  the project, and the risk that the wire mapping and the disk mapping drift apart.
- *Alternative — `pbjson`:* the usual answer for canonical proto JSON in Rust. *Downside:* same
  unapproved-dependency problem, and it still does not solve the comment round-trip, so `plan-core`
  needs its own layer regardless.

---

**4. Moving harness part 2's formats into wave 1 (section 5).** *Gates T3 (wave 1).*

- **(Recommended) take it.** The cost is one extra `xtask` contract PR in week 1; the benefit is that
  AGENTS.md §10 items 1–4 become a runnable command from the first merged PR, and no golden's shape
  is invented under deadline. *Downside:* it puts a third contract PR in wave 1 on a solo reviewer —
  mitigated by the standing rule that the three wave-1 PRs are reviewed in the order T1 → T2 → T3 and
  T3 depends on nothing, so it can wait in the queue without blocking anyone.
- *Alternative — keep the whole of part 2 at the end:* matches the spec's wording exactly. *Downside:*
  six waves of goldens accrete one convention per task, and the runner certifies nothing written
  before it.

---

**5. `Playbook.kind`'s enum values.** *Gates T1.* Item 47 deferred exactly this to the skeleton.

- **(Recommended) `KIND_UNSPECIFIED = 0`, `PLAYBOOK = 1`, `TEMPLATE = 2`, `SAMPLE = 3`**, with the
  verifier rejecting `KIND_UNSPECIFIED` as an E000x. Three values cover the skeleton's library and
  v1.1's published library, and more are additive. *Downside:* if the v1.1 library wants a fourth
  kind with different semantics (say, a rebindable fragment), it arrives as a new value on an
  already-published enum, which is a documentation job rather than a format break — but the
  distinction between `TEMPLATE` and `SAMPLE` is a guess about S6's library and may prove to be one
  concept.
- *Alternative — `PLAYBOOK` and `TEMPLATE` only:* fewer guesses. *Downside:* the sample library
  (spec section 13) then has no tag, and a sample loaded as a playbook is indistinguishable.

---

**6. The plan fingerprint's hash function.** *Gates T1.* `playbook.proto` carries this as a PLACEHOLDER.

- **(Recommended) xxh3-64 over the canonical JSON bytes, under item 48's `STATE_HASH_SEED`, with its
  own domain-separator prefix** — so the project has **exactly one hash function**, which is what
  AGENTS.md §5's "never add a second hash function" asks for. *Downside:* xxh3 is not
  cryptographic, so the fingerprint identifies a plan, it does not authenticate one; if v1.1's
  published API ever wants tamper-evidence, that is a second, explicitly different field rather than
  a change to this one.
- *Alternative — BLAKE3 or SHA-256:* tamper-evident from the start. *Downside:* a second hash
  function and a new dependency, for a v1 property nothing needs (the fingerprint's v1 job is a
  reserved seam).

---

**7. Where the rules table lives, and what `rules_hash` covers.** *Gates T2 (wave 1).* AGENTS.md §12
says tuning values are "versioned in the rules table and stamped into the rules hash"; nothing in the
spec says what that file is.

- **(Recommended) a `gp.v1.RulesTable` message, written on disk as `rules/rules.v1.json` in canonical
  JSON**, loaded by `pharmakos-sim` and hashed with item 48's encoder so `rules_hash` is one of the
  verifier's five inputs by construction. K and B, 10/14/4, `MOVE_COST_PER_TICK`, the repath cap,
  the segment ladder, the CSR cell size and every `$`/`kW` number then sit in one reviewable file
  whose *shape* `buf breaking` already guards. *Downside:* it makes the rules table a contract file,
  so every tuning change during S1 and S2 is a contract PR — which is heavier than it sounds when
  balance work starts. Mitigation: the *shape* is the contract; the *values* are data in the JSON,
  and only shape changes go through `buf breaking`.
- *Alternative — a plain JSONC file with a hand-written schema:* tuning changes stay light.
  *Downside:* a second parser in the sim, no `buf breaking` on its shape, and the verifier needs its
  own reader.
- *Alternative — Rust constants with a generated hash:* simplest. *Downside:* it contradicts AGENTS.md
  §12 outright, and item 54 and item 59 both explicitly require their numbers to be rules-table data.

---

**8. Real HPA\* at the skeleton, or the estimator's contract over a simpler search.** *Gates T7 (wave 3).*
This is the one place two higher sources pull against each other and the plan does not resolve it.
Spec section 17 gives S3 the depth task "HPA\* pathing in the sim and the travel estimator (G2+P3,
±15 %)", and item 71 says the path-identity comparison "joins it **when pathing lands at S3**".
Against that: the wk-18.5 row requires an editor that draws routes with travel times, item 57 makes
the travel number a promise, item 61 makes *never optimistic* part of the contract, and G3′ measured
96.5 % of the gate tick as pathing load.

- **(Recommended) land the mechanism now (T7 as written), and leave the *certification* at S3.** The
  skeleton builds cluster-32 HPA\*, the repair, the oracle, the cap and the estimator, and commits
  the path-hash golden; S3 keeps the ±15 % certification on real code, the corner entrance and the
  endpoint-sweep work as its depth task. *Downside:* it pulls S3's named depth task forward by three
  stages, so S3's row loses most of its content and the schedule above carries an 8-day task the
  milestone table does not name. It also means the path-identity CI comparison lands earlier than
  item 71 assumed, which is additive rather than contradictory but is still a change to a closed
  decision's assumption.
- *Alternative — a flat A\* over the surface graph behind item 61's frozen signature:* keeps the
  contract fixed and lets S3 replace the internals. *Downside:* a flat A\* cannot produce `legs` (the
  abstract-edge count the editor draws the polyline from) or price fog ×1.5 *per abstract edge*, so
  the contract it claims to freeze is partly unimplementable; and the thing G3′ measured as 96.5 % of
  the tick goes unmeasured for the whole stage.
- *Alternative — a straight-line or Manhattan stub:* cheapest. *Downside:* item 61 forbids it — a
  straight-line estimate is **optimistic**, which is the one property the editor's rounding rests on.

---

**9. JSON-RPC parameter casing on the wire.** *Gates T1.* `gateway.proto` carries this as an explicit
PLACEHOLDER: do enum-valued params (`depth`, `detail`) travel as the spec's lower-case strings or as
the enum spelling?

- **(Recommended) lower-case strings on the wire, translated at the boundary** — spec section 12's own
  examples read `verify_plan{depth:"quick"}`, and the gateway is the surface v1.1 publishes to script
  authors. *Downside:* the wire and the schema spell the same value two ways, so there is one
  conversion function to keep tested.
- *Alternative — the enum spelling (`DEPTH_QUICK`):* one spelling everywhere, generated for free.
  *Downside:* it contradicts the spec's worked example and reads badly in a hand-written client.

---

**10. Which three of the eight templates ship.** *Gates T18 and T19 (wave 6); cheap to answer now.*
The spec names eight for S6 and three for the skeleton, and never says which three.

- **(Recommended) Hold & Build, Expand & Mine, Safe Playbook.** The first teaches the beacon, the
  second teaches the route and the economy, and the third is the one the operator must file on a
  timeout anyway — so it is written once and tested twice, and the editor can render "the cost of a
  timeout". *Downside:* no attack-shaped template until S3, which is consistent with combat being S2
  but means the wizard shows nothing aggressive at the demo.
- *Alternative — Hold & Build, Expand & Mine, Fortify Under Threat:* a more varied demo. *Downside:*
  Fortify leans on Defend settings that are S2's, and the safe playbook then has no editor rendering
  even though the operator files it.

---

**11. What `submit_plan` runs at the skeleton.** *Gates T6 (wave 2).* Section 17's wk-18.5 row says
"verifier QUICK"; section 12 says `submit_plan` "always runs FULL" and `verify_plan` defaults to
full. The two are in tension and nothing in the decisions log resolves it.

- **(Recommended) ship `verify_plan{depth}` and `report_hash` complete from day one, with FULL's
  estimate and lint stages present but empty**; submit runs FULL as section 12 specifies and gets an
  honest (short) report. The API shape and the hash contract then never change when S1 and S3 fill
  the stages. *Downside:* a FULL report at the skeleton contains almost nothing beyond QUICK's
  findings, so "FULL" is briefly a promise rather than a depth, and the `report_hash` goldens move
  once when S3 fills the lint stage — a movement that must be explained in that PR.
- *Alternative — submit runs QUICK at the skeleton, FULL from S1:* matches section 17's row literally.
  *Downside:* `verify_plan`'s default and `submit_plan`'s behaviour then disagree with section 12 for
  a stage, and the depth parameter's semantics change under clients that already call it.

---

**12. The `unsafe_code` allow that gdext needs.** *Gates T12 (wave 4).* `[workspace.lints.rust]` sets
`unsafe_code = "deny"` with a comment that `client-gdext` "may need a scoped allow once gdext lands".
The `[workspace.lints]` table is an AGENTS.md §5 contract path.

- **(Recommended) a crate-local `#![allow(unsafe_code)]` in `crates/client-gdext` only**, with a
  comment naming gdext as the reason, raised in T12's PR rather than by editing the workspace table.
  The crate is already walled, `wall-guard` already proves no deterministic crate depends on it, and
  the allow is visible at the top of exactly one file. *Downside:* an `#[allow]` in source is the
  shape AGENTS.md §4.9 warns about for *determinism* lints; `unsafe_code` is not a determinism lint,
  but the precedent needs stating in the PR so nobody generalises it.
- *Alternative — a per-crate `[lints]` override in `crates/client-gdext/Cargo.toml`:* keeps the allow
  out of the source. *Downside:* it is still a lint-table change and reads as one.
- *Alternative — keep `deny` and wrap every gdext call in a shim crate:* strictest. *Downside:* a
  crate that exists only to hold `unsafe`, which the crate map does not have and which "the client is
  thin" does not want.

---

**13. Save and resume in the skeleton, or not.** *Gates T17 (wave 6).* Spec section 3 and section 15
both specify save at Lull boundaries; the wk-18.5 row does not name it.

- **(Recommended) build it (T17, 4 d).** The gateway's private match cache, the frozen segment-end
  snapshot and the rules-hash stamp all exist by then, so the marginal cost is a day of plumbing and
  three days of round-trip tests — and the round-trip test is itself one of the strongest determinism
  assertions in the stage. *Downside:* 4 agent-days on something the milestone row does not name, and
  a save format that S1's and S2's new hashed state will each move.
- *Alternative — defer to S1:* keeps the row literal. *Downside:* the demo cannot be paused and
  resumed, and the save format then arrives on top of one more stage's state rather than the
  minimum.

---

## 7. Owner decisions during the stage

These can be answered as their task approaches. Each names the task that needs it.

**14. The tuning seeds the skeleton cannot start without.** *(T14, wave 5; the map generator's share
at T5, wave 2.)* Sphere radius, placement range, commander speed and arrive radius, respawn delay
growth, BMI, starting `$` (= 2 rounds of BMI), structure and unit costs, `kW` draw per item, core
surplus, Generator output per vent richness, vents per map, revive margin, ore yield per richness,
the fabricator backlog threshold, ruin salvage value, starting force, and the map's footprint.
**(Recommended)** the owner picks a first set in one sitting at the start of wave 2 from the spec's
stated ranges (commander 1–1.5 voxels/s, segment ladder 3/4/5/6/7/8, notebook 4 000 characters), every
value landing as a `rules/rules.v1.json` row carrying `PLACEHOLDER: tuning, owner, <stage>` rather
than as a constant in code. *Downside:* a number people plan against tends to stick; the rules-table
home and the loud label are the mitigation, and spec section 19 re-derives each at its stage.
*Alternative — leave them unset until S1:* keeps them honest. *Downside:* the skeleton has to be
playable, so "unset" means an implicit number somebody wrote in code.

**15. `kW` per unit and the 40 % reserve inside item 65's 190 kW.** *(T5, wave 2.)* Both are
PLACEHOLDERs *inside* the provisional budget, and the map generator needs a ceiling now.
**(Recommended)** take 1 kW per unit and a 40 % reserve as written, in the rules table, labelled
provisional, re-derived at S2's exit exactly as item 65 says. *Downside:* the generator's supply is
sized against a number two of whose three inputs are guesses. *Alternative — defer:* "no number" is
an implicit number nobody wrote down, on a generator already written.

**16. The scenario assertion vocabulary.** *(T15, wave 5.)* T3 freezes the file format with "event
fired" and "hash chain equals". **(Recommended)** extend it once, at T15, when the runner meets real
events — adding at most "event count in range", "state hash at tick N" and "terminal hash". *Downside:*
a format change after producers exist, which is exactly what T3 exists to avoid; it is accepted here
because the assertion *vocabulary* is data inside the format, not the format.

**17. Who owns the mesher's input type.** *(T4, wave 1/2.)* Item 56 says the mesher takes "integer
chunk data in" but not who owns the type. **(Recommended)** the mesher owns a plain `ChunkView<'a>`
over borrowed integer data; `pharmakos-sim` exposes a borrowable chunk view; `client-gdext` performs
the conversion, which is marshalling and so inside the thin-client rule — this keeps `mesher` free of
a sim dependency in either direction and keeps `client-gdext` inside the dependency list the crate map
grants it. *Downside:* one conversion written twice-removed from both ends. *Alternative — the sim owns
it and the mesher depends on the sim:* fewer types. *Downside:* it puts a dependency edge from a walled
crate into the determinism crate, which is the edge `wall-guard` exists to reason about.

**18. Which of the ~35 diagnostic codes the skeleton implements.** *(T6, wave 2.)* Spec section 11
gives families, not a list. **(Recommended)** implement every code the three templates and the v1
vocabulary can actually produce, and leave the rest as catalogue entries with no emitter, so the
catalogue is complete and the coverage is honest and recorded in the catalogue golden. *Downside:* a
catalogue with unreachable entries reads as unfinished. *Alternative — implement only what fires and
omit the rest:* tidier. *Downside:* code numbers are append-only from the day they ship, so omitting
them now makes their numbering an S1 argument.

**19. A provisional playbook size budget.** *(T6 and T8, waves 2–3.)* P1 sets the real number at S3's
exit; the editor's size meter and the verifier need *a* number now. **(Recommended)** a provisional
row in `rules/rules.v1.json`, chosen so FULL stays comfortably under 50 ms on the owner's machine,
loudly labelled provisional. *Downside:* players and templates plan against it and it tends to stick.
*Alternative — no meter until S3:* honest. *Downside:* spec section 13 names the size meter in the
editor, and a meter with no budget is not a meter.

**20. The private match cache's location.** *(T9, wave 3.)* The spec says it is plain local state that
anyone with filesystem access can read, but not where it goes. **(Recommended)** the OS-standard
per-user data directory, one folder per match, with the UI saying it is unencrypted — which is what
the spec means by "enforcement, not cryptography". *Downside:* none material. *Alternative — beside
the executable:* easier for a tester to find and send. *Downside:* an unsigned zip writing into its own
folder fails on a read-only install path.

**21. What happens if the sim slips.** *(Decided in advance; invoked at wave 4 if needed.)*
**(Recommended)** hold the one-crate-one-agent rule; if the sim is more than a week behind at the end
of wave 4, re-open item 70 as an **additive** split — a `math` crate the sim re-exports — which item
70 itself says "is additive then and costs nothing now". *Downside:* it creates the second home for
contract code that item 70 rejected, and `AGENTS.md` §5's "the determinism code" stops being one
nameable path. *Alternative — two agents in `crates/sim`:* faster. *Downside:* it breaks §6 outright
on the crate where a merge conflict is a desync.

**22. The screenshot golden's tolerance.** *(T16 and T20, waves 5 and 7.)* **(Recommended)** G1's
measured shape — mean difference ≤ 0.0039/255 with 0 pixels differing by more than 32, on
Linux/lavapipe only, as a regression alarm rather than a certification. *Downside:* a driver update
can move it, and the alarm then says "look" rather than "this is broken". *Alternative — exact
equality:* unambiguous. *Downside:* G1 measured a 1.14 % of pixels differing at all between a Quadro
and lavapipe with identical geometry, so exact equality would be permanently red.

**23. The perf alarm thresholds.** *(T20, wave 7.)* **(Recommended)** publish the mesher p99, the
tick-minus-pathing mean and the allocations-per-tick count as `::notice::` annotations with **no
threshold at all** in this stage, because AGENTS.md §9 item 11 puts budgets with the gates that set
them (S1's P1, S2's G3′-real). *Downside:* a regression between here and S2 is visible only to
someone reading annotations. *Alternative — set provisional alarms from the spike numbers:* catches
regressions now. *Downside:* it bakes a number no measurement of the real code supports, on hardware
the spikes did not run on.

**24. The zip's shape.** *(T21, wave 7.)* **(Recommended)** one zip per platform containing the Godot
export, the staged cdylib, `gamectl`, the template and sample folders, `LICENSES/`, the credits
attributions and the unsigned-build note; version string `0.1.0-dev+skeleton` until the wk-35.5
release. *Downside:* shipping a CLI invites support questions. *Alternative — game only:*
smaller. *Downside:* `gamectl seat doctor` and `gamectl verify` are what make a tester's bug report
reproducible.

---

## 8. Deliberately left out

**Out because AGENTS.md §11 says so.** Deferred by decision, not by oversight; most have a reserved
proto seam, and filling a seam early is the expensive mistake. No script runtime, script language or
interpreter for user text (no QuickJS, Boa, Luau, Lua, `wasmi`, no mini-language) — the seams
`operator {builtin|script}`, `mandate {builtin|script}`, `program {builtin|script}`,
`author_kind SCRIPT` and the plan fingerprint exist in the proto and **nothing in v1 reads them**. No
MCP server, no `rmcp`, no tool server. No SDK and no published API — the Seat Gateway stays internal
and agent-shaped, and `llms.txt`, the agent guide and published schema docs ship with v1.1. No AI or
LLM seats, no `gamectl connect`, no agent sandbox, no heartbeat, no watchdog hooks. No manual control
of anything — the playbook is the only input during a Push. No `set_flag`, `clear_flag`, `branch`,
`repeat` or flag predicates: their field numbers are reserved in T1 and Load **rejects** them with a
code and a JSON Pointer rather than stripping, which is a T6 test. No player Dispatches or
seat-to-seat text. And no human-vs-human, teams, more than three seats, internet play, dedicated
servers, voting, the web spectator, the RL interface, flowing water, beacon capture, Citations, grid
islands, batteries, the reinforced wall/gate/shield block, timed weapons, operator personalities
beyond Balanced, the director camera, the expert editor view (selectors *do* ship in v1),
translation, the art pass, accelerated player-facing AI-only matches, or over-the-air attacks and
their counters.

**Out because a later stage owns it.** Pulling any of these forward trades "ends playable" for
"almost". **S1:** the full grid and brownout in anger, full Build settings, finite-seam depth,
fabrication from work in hand at full detail, the catch-up dial's tuning, and P1 QUICK's gate.
**S2:** mortar, wall, demolition charge, breach, friendly fire, repair and reclaim drones, full
Survey, commander hunting and bodyguard, the kill-credit *split* (its counters are hashed from T14
regardless), local-elimination beacon death at full depth, the three nightly adversarial scenarios,
and G3′'s real measurement, which sets the per-map power budget. **S3:** the rule list at full depth,
the size meter's real number, the Probation memos and guided first pick, Resonance Spire and licence
plumbing, Save and Load of JSONC files from the editor, P6, verifier FULL's lint catalogue and the
plan-size budget, and — subject to decision 8 — HPA\*'s certification, the corner entrance and the
endpoint-sweep work. **S4:** Radio Mast and mast coverage, Sensor Spire, licence triggers, the award
fund, the **3-way symmetric generator and its fairness suite** (the skeleton ships a seeded map with
the spawn-distance rule, which is what the row asks for), and P7. **S5:** Normal and Hard operators,
seat-blind target spread, the chatter ticker, and P2. **S6:** the full editor (segment clock,
timeline, inline diagnostics panel), eight templates, Save-as-template, the sample library, art
polish and the sound pass. **S7:** highlights, scrub, **shareable recordings** and casual mode.
**Hardening:** signed installers, the macOS release, accessibility and balance playtests.

**Out because a spike measured it and said not to:** section 2.1, each with the number that closed it.

**Out on principle:** copying spike code into `crates/` (T4, T7 and T11 re-write against the real
types with the `spike-end` worktree open); an `#[allow]` on a determinism lint outside a walled
crate; a second hash function; a shadow copy of a `.proto`; regenerating a golden without a written
behaviour change; and bypass-permissions mode, because the contract-file rule depends on the approval
prompts actually happening.

---

## 9. Risks, and the measurement that closes each

| # | Risk | What closes it | Where | If it does not close |
|---|---|---|---|---|
| R1 | The three operating systems stop agreeing on the hash chain once the sim is real rather than a toy | `tests/golden/determinism/expected.hashes.txt` green on the three-OS matrix, 50/50 save/restore, 20/20 fork equivalence, and the **source-text test** that no clock and no `HashMap` reach the crate | T2, extended by every later sim PR | G4's fallback stands: replays guaranteed on the same binary only — but it would be found in week 2, not week 14 |
| R2 | A field is added to sim state and not hashed, producing a desync days later on another OS | The AGENTS.md §4.8 three-places rule enforced per PR, plus the committed goldens moving visibly when state changes | T5, T7, T10, T11, T14 | The nightly three-OS job is the net; the cost is a bisect rather than a rewrite |
| R3 | Pathing is 96.5 % of the tick and its burst behaviour is unmeasured on real code | The **repath-cap sustainability test** — backlog does not grow over a full 8-minute segment at cap 16, with a `sustainable` boolean in the output — plus repath p99, estimate p99 and the endpoint-sweep share reported | T7 | Item 69's cap is re-derived at S2's exit as planned; the sustainability boolean is what tells the difference between "fast" and "not doing the work" |
| R4 | The editor's travel promise is optimistic, which item 61 forbids by contract | **Signed** estimate error asserted never negative at any percentile against walked time on static terrain, plus absolute error p90 within 15 % | T7 | Decision 8's alternatives; the fallback in spec section 16 ("show ranges") stays available but is not taken |
| R5 | gdext on a fresh checkout — G1 lost four CI runs to `extension_list.cfg`, and a silent panic catch turns a crash into a healthy CSV | `--import` as a required pre-step in every job, a **caught-panic counter asserted at 0**, and a **byte-scan of the staged library for `gdext_rust_init`** before a build is declared good | T12, T3 | The cost is measured in CI minutes, not in design — but only if the guards exist before the first Godot job runs |
| R6 | The vista's geometry differs between machines, or the screenshot check silently checks nothing | The mesher's geometry digests compared across OSes from the headless proxy (no GPU), **plus** the 1280 × 720 golden under xvfb + lavapipe — never `--headless`, which selects the dummy driver and parks the coroutine for ever | T4, T16, T20 | The geometry golden alone still catches a mesher regression; the screenshot alarm is the weaker of the two by design |
| R7 | A golden's shape gets negotiated under deadline in the last fortnight | Formats frozen in wave 1 as skipping `xtask ci` steps, with a self-test that a mismatched fixture produces a readable first-difference report and a doctored PNG fails | T3 | Every later task invents its own convention, which is what this ordering exists to prevent |
| R8 | The gateway's security surface has to be reworked when v1.1 publishes it | The surface built and tested **before** any method hangs off it, with the five invariants as named negative tests | T9 | A rewrite of the surface the whole roadmap inherits — the most expensive avoidable rework in the stage |
| R9 | The sim's 52 agent-days on the critical path slip and take the stage with them | T2 publishes the public snapshot/knowledge/rules types on day one so nothing queues behind the sim; every wave's other two tasks are chosen to be unblocked by it; decision 21 governs the slip case | T2, section 4 | Decision 21: an additive `math` crate, not two agents in one crate |
| R10 | Seven-plus contract PRs queue on one reviewer and the float evaporates | One `xtask`/workflow PR open at a time; a named review order in wave 1; every task over 10 agent-days carrying a written split seam; 1.3 weeks of float with its consumers named | sections 3, 4 | The stage slips into the float first and into week 16 second; the early warning is the wave-3 checkpoint |
| R11 | The scenario runner certifies nothing because it arrives last | It arrives at wave 5, one wave before the vista and editor need it, on a format frozen in wave 1 | T3, T15 | AGENTS.md §10 item 4 becomes a week-14 tick rather than a working check |
| R12 | A perf number that cannot vary across platforms is compared across them, giving a green matrix that means nothing | The three-OS job compares only hashes, path hashes and geometry digests; perf figures are per-runner annotations with no threshold | T20 | G3′ §9.17's exact warning — false reassurance, which is worse than no check |

---

*Sources: `docs/spec/pharmakos-spec-v0.6.html` sections 3–17 and 19; `AGENTS.md` §3, §4, §5, §6, §7,
§9, §10, §11, §12; `docs/design/decisions-log.md` §2.7 items 46–71; `docs/spikes/G1-destruction-remesh.md`
§10.12, `docs/spikes/G2-P3-pathing.md` §9.11, `docs/spikes/G3prime-segment-budget.md` §9.17–9.18,
`docs/spikes/G4-determinism.md` §9; `docs/design/handoff.md`; `xtask/src/main.rs`, `proto/**`,
`clippy.toml`, `Cargo.toml` and the `feat/harness-mesher-crate` branch as of 2026-09-14.*
