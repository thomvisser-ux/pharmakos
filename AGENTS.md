<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# AGENTS.md — Pharmakos

Rules for any coding agent (and any human) working in this repository. `CLAUDE.md` includes this
file; there is one set of rules, not two.

Read this whole file before your first edit. If a rule here and the design docs disagree, follow
the precedence order in `docs/design/README.md` (decisions-log §2.7 > spec v0.6 > co-design doc)
and say so in your PR rather than guessing.

**Status of this repository:** the toolchain is installed — rustc 1.98.1 (MSVC host on Windows),
`protoc` 36, `buf` 1.73, `cargo-deny`, `reuse`, Godot 4.7.2 — and `cargo xtask ci` is green:
twelve steps, ten `ok` and two `skipped` with reasons (no goldens, no determinism binary yet),
verified on Windows. The three-OS matrix runs on every pull request and has no hash chains to
compare until the determinism binary lands, so §10's "all three operating systems agree on the
hash chains" is not yet a claim anyone can make. The four stack spikes are closed, frozen at the
`spike-end` tag and deleted from `main`;
their measured results and their lessons for the skeleton live under `docs/spikes/`, and reading the
spike code means `git worktree add ../pharmakos-spikes spike-end`. The walking skeleton is open.
Most crates are still empty placeholders: write files that will be correct when the crate around
them exists, and mark any value you had to guess with a `PLACEHOLDER` comment naming who fixes it
and when.

---

## 1. The design in one paragraph

Pharmakos (lockup: *PHARMAKOS: THE SEALED ORDER*; codename AirGap) is an open-source voxel strategy
game about orders you cannot take back. Up to three seats (at most one human, the rest built-in)
share a local match: during the planning **Lull** each seat seals a **playbook** — typed declarative
data, not a script — which the **verifier** (the Ledger's seal inspection) accepts or rejects; then
the **Push** plays out for 3 to 8 minutes with nobody in control, watched like a movie. The only way
to change anything is to walk the **commander** to a **beacon** and interface on site: *touch to
change*. Beacons are the unit of power and carry **mandates** (Build, Defend, Attack, Mine, Survey);
units and buildings run **programs**; money is `$` and power is `kW`; the world is destructible
voxels under a permanent ash sky. Radio exists only once a Radio Mast is earned, carries data and
never control, and is treated as untrusted input. Everything is deterministic, seeded and hashed, so
a match replays bit-for-bit on every platform.

## 2. The v1 rule

**Everything that executes is Rust. Everything the player touches is data.**

Three levels of behaviour ship built-in and native: the commander's **operator** (generates a
playbook, files the safe playbook on a miss, executes whatever the seat sealed), the beacons'
**mandates**, and the units' and buildings' **programs**. The player authors exactly one thing: the
playbook, a JSONC file (canonical `gp.v1` proto JSON plus comments) written in the in-game editor or
by hand.

Consequences that bind every change you make:

- No script engine, no script runtime, no interpreter for user text. Not QuickJS, not Boa, not Luau,
  not `wasmi`, not Lua, not a mini-language of your own.
- No MCP server (`rmcp` is not a dependency), no SDK, no published API, no AI/LLM seats.
- The Seat Gateway is internal in v1 but stays *agent-shaped*, because v1.1 publishes it. Do not
  bake in assumptions that only the editor will ever call it.
- Script seams exist in the proto as oneofs with only the `builtin` arm implemented
  (`operator {builtin|script}`, `mandate {builtin|script}`, `program {builtin|script}`,
  `author_kind SCRIPT`, a plan fingerprint). Reserve them; never read them.

## 3. Crate map

One Cargo workspace at the repository root. Two languages only: Rust, and GDScript for views and
editor UI. Nothing else executes. Directories are under `crates/`; package names are
`pharmakos-<dir>` (the `gamectl` binary is just `gamectl`).

| Crate | Does | May depend on |
|---|---|---|
| `crates/proto` — `pharmakos-proto` | Generated prost types for `gp.v1` (playbooks, templates) and `gp.api.v1` (gateway), the canonical JSON mapping, and the voxel run-length codec (`chunk_rle`) the gateway's view feed encodes with and the client decodes with. The single schema source. Licensed `MIT OR Apache-2.0`, unlike the rest. | `prost` |
| `crates/sim` — `pharmakos-sim` | The deterministic sim: 20 Hz fixed tick, integer maths, SoA tables, 32³ copy-on-write chunks, runner (Lull/Push/recap), playbook interpreter, mandates, programs, Quartermaster, power grid, combat, kill-credit counters, pathing/ETA, map generation, snapshot/restore, replay, per-tick xxh3 state hash. `fork` lives **here and nowhere else**, behind `feature = "research"`. | `pharmakos-proto`, and the determinism crates (`xxh3`, `imbl`, `postcard`/`rkyv`, pathfinding primitives) |
| `crates/plan-core` — `pharmakos-plan-core` | Playbook core: canonical form, JSONC round-trip (comments survive), JSON Patch, `render_plan` prose, interface-time and `$`/`kW` arithmetic, travel estimates. In process with the gateway. | `pharmakos-proto`, `pharmakos-verifier`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/verifier` — `pharmakos-verifier` | Seal inspection: decode → structure → resolve → semantics (QUICK) → estimate → lint (FULL). Diagnostic catalogue, `report_hash`. | `pharmakos-proto`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/operator` — `pharmakos-operator` | The built-in operator: templates + utility scoring, the safe playbook, the wizard's suggestions, Easy/Normal/Hard. An ordinary gateway client with no privileged reads: from T18 it reaches the match only through a call closure over `gp.api.v1` JSON-RPC, which the gateway binds to that seat's own in-process token (`serve::InProcessSeats`) and `gamectl host` adapts to `serve::Operators` — a `BuiltInSeat` plays a seat; an `Advisor` returns a human seat's safe playbook and suggestions, which the host files — and it reads the public rules text for its tuning rows (decisions-log item 111). Filing on a miss is the gateway's (`begin_push`) and executing a sealed playbook is the sim's. | The library: `pharmakos-proto` only. Its tests may add `pharmakos-gateway` as a dev-dependency (T18 needed none); an operator test that hosts a match lives in `crates/gamectl/tests` |
| `crates/gateway` — `pharmakos-gateway` | Seat Gateway: JSON-RPC over a localhost WebSocket, tokens, scopes, fog filter, rate limits, event bus, snapshots, private match cache, saves and resume, and the private replay's inputs. Hosts the match. | `pharmakos-proto`, `pharmakos-plan-core`, `pharmakos-verifier`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/gamectl` — `pharmakos-gamectl` (bin `gamectl`) | CLI: `verify`, `schema`, `docs`, `scenario run`, `seat doctor`, `host`. No `connect` in v1. `scenario run` hosts a headless match and `host` serves one to the Godot client over its own stdio pipe (the config line in on stdin, the announce line with the port and the tokens out on stdout, exit when stdin ends), which is why the gateway is on the list. From T18, `host` links the built-in operator and adapts its client to the gateway's in-process seats (decisions-log item 111); from T18b, `scenario run` seats a `builtin` seat with Easy through `serve::InProcessSeats` (decisions-log item 113). | `pharmakos-proto`, `pharmakos-plan-core`, `pharmakos-verifier`, `pharmakos-gateway`, `pharmakos-operator`, and `pharmakos-sim` *only* as `default-features = false`, because the gateway's API is written in the sim's types. `gamectl` drives a match only through the gateway's `Host` and `Surface`, never through the sim's `Runner` (decisions-log item 109) |
| `crates/mesher` — `pharmakos-mesher` | The **walled** greedy mesher: integer chunk data and `.vox` models in, vertex and index buffers out, under the per-frame upload budget the client applies. Links without gdext, so the headless CPU proxy and the CI geometry check reuse it. | `dot_vox` later; nothing from the sim. Never depended on by `sim`, `plan-core`, `verifier`, `operator`, `gateway` or `gamectl` — `cargo xtask wall-guard` fails the build over it (`WALL_GUARDED_PACKAGES`, §4.9) |
| `crates/client-gdext` — `pharmakos-client-gdext` | **Thin** gdext bridge (`cdylib` + `rlib`): marshals gateway calls and mesh buffers between Godot 4.7 and Rust. It marshals; it does not decide, and rule 4 names the pieces that schedule or draw without deciding. | `godot` (gdext), `pharmakos-proto`, `pharmakos-mesher` |
| `xtask` | `cargo xtask ci` and friends. Dev-only, never shipped, std-only, no dependencies. | nothing |

> **Internal boundaries — decided (decisions-log items 56 and 70).** There is **no `math` crate**:
> the integer newtypes (`Fx` Q16.16, `Sq` Q32.32, `Angle` u16 + LUT, HP, `$`, `kW`, `Tick`, `Ms`),
> the RNG streams, the canonical encoding, the state hash, HPA\* and the map generator stay as
> modules inside `pharmakos-sim`. `plan-core`, `verifier` and `gateway` already depend on the sim
> with `default-features = false` for exactly the snapshot and knowledge types such a crate would
> split out, so it would add a crate without removing a dependency edge — and the determinism
> contract stays in one crate, which is what makes "the determinism code" a nameable contract path
> for §5 and for `cargo xtask ci`. Recorded honestly: the verifier and plan-core compile the whole
> sim crate for a handful of types, which costs build time and keeps the sim's stepping API within
> reach; the research-guard and the "no dry runs" review rule (rule 2 below) are the mitigation, not
> the type system. Revisit only if the skeleton finds the verifier's build time or its reach into
> the stepping API to be a real problem — the split is additive then (a `math` crate the sim
> re-exports) and costs nothing now. **The greedy mesher is the one piece that did move:** it is its
> own walled crate, `crates/mesher`, because the client must use it without reaching the sim and
> because it must link without gdext. Do not split the workspace further on your own initiative.

### Dependency rules that CI enforces

1. **The research-feature ban.** `fork` exists only behind `pharmakos-sim`'s compile-time
   `research` feature, which is defined in that crate and in no other. Release builds never enable
   it; CI builds both configurations. `plan-core`, `verifier`, `operator`, `gateway` and `gamectl` must
   never reach it — not in `[dependencies]`, not in `[dev-dependencies]`, not through a default feature,
   not transitively. They declare no `[features]` section of their own, and any dependency they take
   on the sim reads `pharmakos-sim = { workspace = true, default-features = false }`.
   `cargo xtask ci` walks `cargo tree -e features` and fails the build if the feature reaches any of
   the five.
2. **No dry runs.** `plan-core` and `verifier` may estimate — pathfinder travel over known terrain,
   interface-time arithmetic, `$`/`kW` projection, placement legality, selector previews, mast
   coverage. They may never step or fork the sim, run mandates, programs, combat or construction,
   model enemy behaviour, or evaluate rule conditions over a projected future. Prefer depending on
   the sim's snapshot and knowledge types only; if you find yourself wanting its stepping API, the
   design is wrong — stop and ask. The match itself is stepped and sealed in one module, the
   gateway's `host.rs`. No handler steps or seals, *except* the admin-scoped control handlers in
   `crates/gateway/src/surface/control.rs`, which drive only the live match through `Surface`'s
   driving methods and may name none of `Runner`, `Host`, `World`, `host_mut`, `seal_plans`,
   `seal_playbook`, `snapshot` or `.clone()`; `crates/gateway/tests/confinement.rs` is the guard
   (decisions-log item 107). The in-process seats are host-side the same way:
   `Surface::register_in_process` and `Surface::file_advice` are reached only from `serve.rs`,
   through `serve::InProcessSeats`, and the same test's `no_handler_can_file_advice` is the guard
   (decisions-log item 111). So is the restore: `Host::resume` is the one restore, and `.restore(`
   stays in `host.rs` (the same test's `the_match_is_stepped_in_one_module`); the save seams —
   `Save::parse`, `Surface::resume`, `Surface::take_persistence` and `Surface::lull_save` — are
   reached only from `serve.rs`. Their guard is the same test's
   `no_handler_reads_the_save_or_the_replay`, with its helper `surface_calls_no_save_seam`: it keeps
   them out of every handler module and every dispatch arm, and exempts their definers (`cache.rs`,
   `save.rs`, `surface.rs`) and `host.rs` as well as `serve.rs` (decisions-log item 113).
3. **The operator is not privileged.** `operator` sees the world only through `gp.api.v1` calls —
   same snapshot, same verifier, same submit path as the human — made through a closure the gateway
   binds to that seat's own token, plus the public rules text every client pins. Its library names
   no sim or gateway type and depends on `pharmakos-proto` alone; its tests may use
   `pharmakos-gateway` as a dev-dependency, and an operator test that hosts a match lives in
   `crates/gamectl/tests` (decisions-log item 113). An in-process seat (a built-in seat, or the
   advisor whose safe playbook and suggestions for a human seat the host files) goes through the
   same door, audit and fog filter as a socket, with its own rate limit
   (`limit::IN_PROCESS_LIMITS`). An advisor's token is narrower than a seat's twice over: it never
   holds `plan.submit`, and `serve.rs`'s `ADVISOR_METHODS` allow-list refuses every write and every
   read of the seat's drafts. Its `get_briefing` still carries the seat's notebook; the operator
   never reads it, and a test pins that (decisions-log item 111).
4. **The client is thin.** `client-gdext` contains marshalling and nothing else: no rules, no time
   arithmetic, no validation, no gameplay decisions. The editor "runs no validation or time maths of
   its own" — it asks the gateway. GDScript is views and editor UI only. The pieces of this walled
   crate, and of the Godot scripts, that schedule, draw or name without deciding are named here so
   nobody generalises them: the pacer's clock in `pacer.rs` (speed, skip, the host-clock report and
   the Lull's countdown, the FULL-verify idle timer); the connection scheduling in `rig.rs` (call
   order, a seat's rate budget, Ready waiting behind a submission, `end_lull` once every seat is
   ready or the countdown is spent); the route-target reader in `editor.rs` that finds a route's
   targets in the player's file to draw them; the map menu in `godot/scripts/editor.gd`, which
   routes on the view's own `owner` field; from T19 PR 2, the match-id helper in
   `godot/scripts/host_link.gd`, which reads the clock once to name a match and computes nothing
   with it (decisions-log item 113); and the lobby in `godot/scripts/lobby.gd`, which remembers
   the config line once the host announces and forgets it when the rig reports the match ended
   (decisions-log item 114). The client may compose a JSON Patch from a click; a verdict, a
   travel time, a legality answer, a Fix and the patched text (`patch_plan`'s answer) still come
   from the gateway. The gateway's JSON-RPC answers are not canonical `gp.api.v1` JSON — they carry
   a `_status` footer and lower-case enum values (decisions-log item 80) — so every client of them,
   `client-gdext` and the operator alike, strips the footer and translates enum values
   (`pharmakos_proto::scope::from_wire_name`); canonical proto JSON is the playbook file's format,
   not the wire's (decisions-log items 110 and 112).
5. **Banned dependencies.** `bincode` (unmaintained), `cordic`, `hierarchical_pathfinding` (stale),
   `rmcp`, `wasmi`, any scripting engine. Approved: `prost`, `buf` (tooling), `rkyv`/`postcard` (with `serde` as postcard's derive companion only — decisions-log item 88),
   `xxh3`, `imbl`, `dot_vox`, pathfinding primitives under our own HPA\*, `getrandom` (the gateway's seat tokens only — item 99), `lexopt` 0.3 (`gamectl` only — item 105). Adding a dependency that
   is not on the approved list is an owner decision — open a PR and stop (§5).

## 4. Determinism rules

The sim is a pure function of (map seed, playbooks, rules hash). Two machines running the same
binary on Windows, Linux and macOS must produce the *same per-tick xxh3 hash chain*. Every rule below
exists because breaking it produces a desync that shows up days later as an unreproducible replay.

The lints are not advisory. `cargo xtask ci` denies warnings; a `#[allow]` on a determinism lint
needs a comment saying why and is reviewed as a contract change.

### 4.1 Integer newtypes, never bare numbers

```rust
// DO — a unit type per quantity; the compiler stops you mixing them.
/// Position along one axis, Q16.16 voxels.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Fx(i32);

impl Fx {
    pub const ONE: Fx = Fx(1 << 16);

    /// One of the sim math module's audited widening casts (§4.3): i16 → i32 cannot
    /// truncate, and a const fn cannot call `i32::from`.
    #[allow(clippy::as_conversions)]
    pub const fn from_voxels(v: i16) -> Fx { Fx((v as i32) << 16) }

    pub fn checked_add(self, rhs: Fx) -> Option<Fx> { self.0.checked_add(rhs.0).map(Fx) }
}

// DON'T — a bare i32 "in some unit" is how Q16.16 gets added to whole voxels.
pub fn move_unit(pos: i32, speed: i32) -> i32 { pos + speed }
```

Overflow checks stay **on in every profile**, release included. Do not "fix" an overflow panic by
widening blindly: decide whether the value should saturate, clamp or be rejected, and say so in
code — `checked_*` with a typed error, `saturating_*` where the cap is a real game rule.

### 4.2 No floats

```rust
// DON'T — f32 rounds differently across targets, and `sqrt` is the classic offender.
let d = ((dx * dx + dy * dy) as f32).sqrt();
if d < radius { engage(); }

// DO — compare squared distances in Q32.32. Range checks use r², never a square root.
let d2: Sq = Sq::between(a, b);
if d2 <= radius.squared() { engage(); }

// DO — angles are u16 with a 4096-entry lookup table; no trigonometry at run time.
let heading = Angle::from_delta(dx, dy);          // LUT
let step = TRIG.cos_sin(heading);                 // fixed-point pair
```

Lint: `clippy::float_arithmetic` is denied workspace-wide. Floats are legal only inside a walled
crate — see §4.9.

### 4.3 No `as` casts

```rust
// DON'T — silently truncates, and the bug only appears at 65 537 HP.
let hp: u16 = damage as u16;
let idx: usize = beacon_id as usize;

// DO — a conversion is a decision; make it one.
let hp: u16 = u16::try_from(damage).map_err(|_| SimError::HpOutOfRange { damage })?;
let idx: usize = beacon_id.index();   // newtype method, documented and bounded
```

Lint: `clippy::as_conversions` is denied. The handful of legitimate widening casts live in the
sim's math module behind named constructors, each with an `#[allow]` and a comment.

### 4.4 No `HashMap` / `HashSet` in sim, plan or gateway state

```rust
// DON'T — iteration order depends on RandomState, so the hash chain diverges per process.
use std::collections::HashMap;
let mut units_by_beacon: HashMap<BeaconId, Vec<UnitId>> = HashMap::new();
for (beacon, units) in &units_by_beacon { settle(beacon, units); }   // order is luck

// DO — a dense table indexed by id, or an ordered map.
let mut units_by_beacon: BTreeMap<BeaconId, SmallVec<[UnitId; 8]>> = BTreeMap::new();
// or, for persistent snapshot-friendly state:
let roster: imbl::OrdMap<BeaconId, Roster> = ...;
```

Lint: `clippy::disallowed_types` with `HashMap`, `HashSet`, `RandomState` in `clippy.toml`. If you
genuinely need a hash map in a presentation cache, use an ordered map anyway — the cost is noise,
the bug is a desync.

### 4.5 No wall-clock time

```rust
// DON'T — anything reading the host clock makes the sim depend on how fast the machine is.
let now = std::time::Instant::now();
if now.duration_since(start) > Duration::from_secs(3) { flee(); }

// DO — game time only. Times in playbooks are game milliseconds, never ticks.
if self.tick >= self.started_at + Ms(3_000).to_ticks() { flee(); }
```

Lint: `clippy::disallowed_types` covers `Instant`, `SystemTime`, and `std::time::*` clocks, and
`clippy::disallowed_methods` covers the readers (`Instant::now`, `SystemTime::now`). Wall time is
legal in one place only: a crate on the wall's list (§4.9), where the allowance is passed on the
command line by `cargo xtask clippy`. The Lull timer is a UI/host concern the sim never reads, so it
belongs on that side of the wall like any other clock. Performance measurement in wall-clock time
goes behind the wall too: `cargo xtask clippy` lints a bench target like the crate that owns it, so a
bench inside a deterministic crate reports *counted work* — steps, nodes, components — and a
millisecond figure comes from a walled crate driving the sim from outside (see `clippy.toml`'s
reason strings). There is no `perf` feature and no per-module escape hatch — if you think you need
one, that is a contract change, and §5 says how to raise it.

### 4.6 Ordered iteration, always

```rust
// DON'T — collecting intents from tasks or channels and applying them in arrival order.
let intents: Vec<Intent> = rx.try_iter().collect();
for i in intents { apply(i); }

// DO — establish a total order before anything mutates the world.
let mut intents: Vec<Intent> = gather();
intents.sort_unstable_by_key(|i| (i.seat, i.beacon, i.unit, i.kind_ord()));
for i in intents { apply(i); }
```

Never parallelise a phase that writes sim state (no `rayon`, no `par_iter` inside the tick). Never
sort by a float key. Ties are broken by explicit ids — the spec's convention is *ties to the lowest
seat id*, and kill-credit shares are apportioned as integers by largest remainder on that rule.

### 4.7 Split seeded RNG streams

```rust
// DON'T — one global RNG, so adding a single roll anywhere shifts every later roll.
let x = rand::thread_rng().gen_range(0..10);

// DO — a stream per purpose, derived from (match seed, stream, tick, seat).
let mut rng = self.rng.stream(Stream::CombatSpread, self.tick, seat);
let spread = rng.range_i32(-SPREAD, SPREAD);
```

Streams are declared in one enum in the sim's math module. Adding a stream is additive and safe;
reordering the enum or reusing a stream for a second purpose is a determinism change (§5). The
verifier, the operator and the map generator are deterministic too: the operator's seed comes from
(match, seat, round) and its budget is measured in evaluation units, never wall time.

### 4.8 Per-tick state hash — and hash everything you add

```rust
// Every tick, after the world settles.
let h: u64 = world.state_hash();          // xxh3 over the tables in declared order
self.hash_chain.push(h);

#[cfg(debug_assertions)]
debug_assert_eq!(h, golden[self.tick.index()], "desync at tick {}", self.tick.0);
```

**The rule that catches most desyncs:** any field you add to sim state must be added to *three*
places in the same PR — the state hash, the snapshot/restore round-trip, and the golden files. A
field that affects behaviour but is not hashed is a latent desync, and CI will only catch it once
two operating systems disagree. That includes the per-seat kill-credit counters (at most three per
asset) and their largest-remainder apportionment: they are hashed state like everything else.
Widening the snapshot has one more consequence to plan for: the fixture snapshot is one of
`report_hash`'s inputs, so every verifier report golden moves with it. Re-bless them in the same
PR and say why (decisions-log item 109). A change to how an existing hashed column is *computed*
moves the chains just as a new field does, with no field added: the PR re-blesses every chain and
golden it moves and names the rule that moved them (decisions-log item 114).

For the chunk store there is a **fourth** place: a voxel write must *mark its chunk* so that
`settle` refreshes the chunk's digest. Miss the mark and the bytes change while the digest does
not — the hash is stale rather than missing, no golden moves, and nothing goes red until a replay
disagrees. Only `set` and `crater` may reach a chunk after generation, and both mark; a new write
path joins that pair and never bypasses it.

### 4.9 The wall is a crate boundary

Floats and wall-clock time are permitted only in the walled presentation/solve layer — rendering,
meshing, camera, UI easing, the Lull timer, and any offline solver whose output is converted to
integers before it is used.

**The wall is a crate boundary, not an `#[allow]` sprinkled through the tree.** Clippy has no
per-path allow-list, so the mechanism is this and nothing else:

- The walled crates are named in one place: `WALLED_PACKAGES` in `xtask/src/main.rs` (today:
  `presentation`, `solve`, `client-gdext`, `mesher`; only the last two exist). `clippy.toml`'s
  header repeats the list for readers.
- `cargo xtask clippy` pass 1 lints the whole workspace *minus* those crates with the full deny set;
  pass 2 lints those crates with `-D warnings` plus the float, cast, hash-map and clock allowances,
  **passed on the command line**. Nothing in the source asks for the allowance. The wall lifts
  exactly the lints in `WALL_ALLOW` (`xtask/src/main.rs`) and nothing else: the panic lints
  (`indexing_slicing`, `unwrap_used`) and the rounding lint (`integer_division`) stay denied inside a
  walled crate too, so a walled crate still reads slices through `get` and divides through
  `checked_div`.
- `cargo xtask wall-guard` then fails the build if `sim`, `plan-core`, `verifier`, `operator`,
  `gateway` or `gamectl` depends on a walled crate, transitively included. That list is `WALL_GUARDED_PACKAGES`
  in `xtask/src/main.rs` — the research guard's five crates plus the sim, which cannot join the
  research guard because the sim is the crate that *defines* the `research` feature. The sim is on
  the wall's list because it is the crate that owns hashed state, so it is the one the wall exists
  to protect. That dependency edge is what makes the allowance safe, and it is only visible to CI
  because the wall is a crate.
- Adding a crate to `WALLED_PACKAGES` widens the allowance for that whole crate, so it is a contract
  change (§5) and needs owner approval.

Two planks hold inside a walled crate as well:

- It never writes sim state, and nothing in the tick calls into it.
- Anything crossing back into the sim crosses as an integer newtype, converted at a named boundary
  function.

What this rules out: a float or clock module *inside* `pharmakos-sim` (or any other deterministic
crate) with a crate-local or module-local `#![allow(clippy::float_arithmetic)]`. Pass 1 lints that
crate with the deny set, but an inner `#![allow]` would win, `wall-guard` could not see it because
there is no dependency edge to see, and CI would go green over a determinism hole. A `#[allow]` on a
determinism lint outside a walled crate is exactly the workaround §5 forbids.

## 5. Contract files need owner approval: open a PR and stop

These files are contracts. Churn in them costs more than any feature they enable, so they are built
to full quality once and changed only with the owner's explicit approval.

- `proto/**/*.proto` and anything generated from them (`gp.v1`, `gp.api.v1`), `buf.yaml`,
  `buf.gen.yaml`, `buf.lock`
- Lint configuration: `clippy.toml`, workspace `[lints]` tables, `deny.toml`, `rustfmt.toml` *if one
  is ever added* (there is none today — formatting is rustfmt's defaults for edition 2024, which is
  also what `scripts/pre-commit.sh` runs), and the `[profile.*]` blocks that keep
  `overflow-checks = true`
- Determinism code: the state hash, the RNG stream enum, the fixed-point types, the snapshot and
  replay formats, the tick loop, the `research` feature and the `fork` implementation
- The formats a match leaves on a player's disk: the save container (`save.json`,
  `crates/gateway/src/save.rs`); the private replay's inputs — `match.json` (the seed, the settings
  and the rules hash, written by `crates/gateway/src/cache.rs`),
  `seats/<seat>/sealed/<round>.jsonc`, and `replay/<round>.hashes.txt` in the scenario hash-file
  format, which therefore lives on players' disks as well as under `tests/golden`; the audit log's
  leading sequence and tick fields, which a resume reads back (`MatchCache::continuation`); and,
  from T19 PR 2, the config line the lobby remembers in `user://last_match.txt`: the six
  `serve::Config` fields that `serve::ConfigLine` reads, without `resume`, and no token
  (decisions-log items 113 and 114)
- CI: `.github/workflows/**`, `xtask/**`'s definition of what `ci` runs, golden-file *formats*
- Licensing: `LICENSES/**`, per-directory `LICENSE` files, the REUSE manifest
- `AGENTS.md`, `CLAUDE.md`, and the design docs under `docs/design/`

**The rule for an agent:** if your change touches one of these paths, make the change on a branch,
open a PR describing exactly what the contract change is and why, and **stop**. Do not merge, do not
self-approve, do not work around the file (a `#[allow]` that defeats a determinism lint, a shadow
copy of a `.proto`, a second hash function) — a workaround is a contract change wearing a disguise.
Regenerating goldens counts: say in the PR *why* the hashes moved, and never regenerate them to make
a red test go green without explaining the behaviour change that moved them.

Proto rules that hold regardless: Protobuf is the single source of truth; fields are added, never
renumbered or reused; reserved field numbers stay reserved; `buf breaking` runs in CI in
`WIRE_JSON` mode; submitted playbooks with unknown fields are rejected.

## 6. Parallel agents

Review is the bottleneck, so parallelism is capped by what the owner can review, not by what the
machine can run.

- **At most 2–3 agents at once.** If you were spawned as a fourth, say so and wait.
- **Each agent works in its own git worktree**, never in the owner's checkout:

  ```sh
  git worktree add ../pharmakos-<task> -b feat/<crate>-<task>
  # work, commit, push, open a PR from that branch
  git worktree remove ../pharmakos-<task>     # when the PR is merged
  ```

- **One crate, one agent.** Two agents never edit the same crate at the same time. If your task
  needs a change in a crate another agent owns right now, stop and ask for the change to be
  sequenced — do not "just add a small helper" over there.
- Rebase on `main` before opening a PR. Never force-push `main`, never rewrite a pushed branch
  someone else is reviewing.
- State in the PR which crates you touched, so the next agent can pick a disjoint set.
- Agents iterate until CI is green on their own branch; the owner reviews the stage demo, not the
  intermediate commits.

## 7. Security rules

The game's premise is that trust is engineered rather than assumed. These rules are the code's share
of that premise.

- **Localhost only.** The Seat Gateway binds `127.0.0.1` and `::1` and nothing else, with `Host` and
  `Origin` checks on the WebSocket upgrade. No `0.0.0.0`, no LAN convenience binding, not even
  behind a flag.
- **Per-seat tokens.** 256-bit tokens tied to a match and a seat, revocable from the lobby. Scopes:
  `observe`, `plan`, `plan.submit`, `docs`, `spectate.nofog`, `admin`. A seat token can *never* hold
  `spectate.nofog` — only a separate spectator token can. `admin` covers lobby and match control and
  can never read another seat's playbooks, drafts or knowledge. Fog is a per-match server-side
  policy applied by the gateway's fog filter, so no token is reissued mid-match.
- **Secrecy.** Playbooks, drafts, the seat notebook and the private replay cache never leave the
  gateway for another seat. On a local host this is enforcement, not cryptography: the private match
  cache is plain local state, and the spec says so out loud. Do not claim more.
- **No filesystem or network access through playbooks.** A playbook is data with a closed
  vocabulary. There is no verb that reads a path, opens a socket, shells out or loads code, and none
  may be added. The template/sample library is a local folder the gateway *reads* to list and
  instantiate; nothing in it executes.
- **No network telemetry.** Playtest mode is opt-in and local: "Export playtest bundle" writes a
  file the tester sends manually, containing survey, metrics, recording and the diagnostics log
  only — never playbooks, drafts, the notebook or the private replay. Networked telemetry is
  roadmap and needs opt-in; do not add an HTTP client to any shipped crate.
- **Never commit keys or tokens.** No API keys, no seat tokens, no signing certificates, no
  `.env` with secrets. Seat tokens are generated per match and live in the private match cache;
  signing material lives in CI secrets. The game never holds a provider key — in v1 there is no
  provider, and in v1.2 the live conduit still holds none.
- **Never run an agent in bypass-permissions mode against a real seat**, and do not use it in this
  repository. The contract-file rule in §5 depends on approval prompts actually happening. Game
  tools are reached through pre-approved allow rules; bypass mode is not a shortcut, it is the
  removal of the mechanism.
- Rate limits and an audit log on the gateway are part of the feature, not a later hardening task.

## 8. Commits

- **Conventional commits.** `type(scope): summary`, imperative, lower case, no trailing period.
  Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
  Scope is the crate or area: `feat(sim):`, `fix(verifier):`, `docs(design):`, `ci(determinism):`.
  A breaking contract change carries `!` and a `BREAKING CHANGE:` footer — and still needs §5
  approval.
- **DCO sign-off on every commit.** `git commit -s` adds
  `Signed-off-by: Name <email>`; keep it as the last line. The `dco` job in
  `.github/workflows/ci.yml` walks every commit in the pull request and fails without it. No
  sign-off, no merge.
- **SPDX headers on every file**, REUSE-style, with a per-directory `LICENSE` and a manifest:

  ```rust
  // SPDX-FileCopyrightText: 2026 Pharmakos contributors
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```

  Which licence a file gets depends on where it lives:

  | Directory | Licence |
  |---|---|
  | game code — sim, client, operator, gateway, gamectl, xtask | `GPL-3.0-or-later` |
  | `proto/`, generated JSON Schema, `docs/`, `llms.txt`, example playbooks, the template library `library/` | `MIT OR Apache-2.0` |
  | art and audio assets | `CC-BY-SA-4.0` |

  Third-party assets keep their own SPDX line and their entry in the credits screen (CC-BY
  attribution is a shipping requirement, not a nicety). Audio comes from CC0/CC-BY sources only.
- An agent-written commit may add a `Co-Authored-By:` trailer above the sign-off. The sign-off
  itself must be a real identity that can make the DCO's assertion.

## 9. `cargo xtask ci`

One command. If it is green locally it is green in CI; if it is not, that is a bug in `xtask`, not a
reason to run something else.

```sh
cargo xtask ci            # everything below
cargo xtask ci --quick    # fmt, clippy, unit tests — the inner loop
cargo xtask ci --fix      # rustfmt and the machine-applicable clippy fixes
```

> **Status.** `xtask/src/main.rs` has been run and is green: its twelve steps cover items 1–9 below,
> and the steps whose inputs do not exist yet (goldens, the determinism binary) report `skipped`
> with a reason rather than `ok`. Items 10 and 11 are **not** in `xtask` today: scenarios are
> harness part 2 and currently live only in `.github/workflows/nightly-scenarios.yml`, and the perf
> budgets arrive with the gates that set them. The owner signs off the final step list, and the
> numeric budgets are Tuning values that the gates in spec section 16 set.

1. **Format** — `cargo fmt --all --check`.
2. **Lints** — `cargo clippy --workspace --all-targets -- -D warnings`, run once with default
   features and once with `--features research`. Never `--all-features` (it would enable `research`
   everywhere and hide the ban). Determinism lints: float arithmetic, `as` conversions, disallowed
   types (`HashMap`, `HashSet`, `Instant`, `SystemTime`), and the walled-module allowance audit.
3. **Dependency policy** — the research-feature ban (§3 rule 1) by walking `cargo tree -e features`;
   the no-dry-runs ban (`plan-core` and `verifier` never enable `research` and never reach the sim's
   stepping API); the banned-crate list; `cargo deny` for licences and advisories.
4. **Profiles** — `overflow-checks = true` in every profile, release included.
5. **Tests** — `cargo test --workspace`, with and without `research`.
6. **Determinism** — replay a fixed set of seeded matches headless and compare the full per-tick
   xxh3 hash chain against golden files; save/restore round-trips hash-identically; in the research
   build, fork equivalence. The CI matrix runs Windows, Linux and macOS and compares the chains
   across all three (gate G4).
7. **Goldens** — JSONC playbooks round-trip byte-for-byte through `plan-core`'s canonical form
   with comments intact; verifier `report_hash` goldens; the diagnostic catalogue; `render_plan`
   prose. Golden diffs must be human-readable.
8. **Schema** — `buf lint`, and `buf breaking` against `main` in `WIRE_JSON` mode (both run from the
   workspace root with `proto` as the input, because buf resolves a `.git#…` reference relative to
   the invocation directory); a second comparison against the last release tag is added when v1.1
   publishes `gp.api.v1`. Generated JSON Schema and `get_schema`/docs output are regenerated and
   must match what is committed (so they cannot drift).
9. **Licensing** — REUSE check: every file has an SPDX header, every directory a `LICENSE`, the
   manifest is complete.
10. **Scenarios** *(harness part 2 — not in `cargo xtask ci` today)* — `gamectl scenario run` over
    the committed scenario files (map seed + playbooks + assertions on events and hashes). Headless
    Godot screenshots for the visual checks. Until `gamectl scenario run` exists, the only scenario
    runner is `.github/workflows/nightly-scenarios.yml`, which is itself gated off until S2.
11. **Perf budgets** *(added per gate, not in `cargo xtask ci` today)* — the G3′ CI budget check
    (tick p99 within budget) lands with S2's exit measurement, and the verifier's QUICK ≤5 ms p99 and
    FULL ≤50 ms p99 at the playbook size budget (P1) land with the verifier's gate.

Nightly, additionally: the three adversarial scenarios (§10) on a fixed seed set, and the fuzzer
(10 000 generated playbooks, no panic).

## 10. Definition of done for a stage

A stage ends **playable**, goes through every layer (sim → verifier → gateway → operator → editor →
view), and is done only when all of this holds:

1. `cargo xtask ci` is green on the CI matrix — all three operating systems agree on the hash chains.
2. **Determinism hashes**: the stage's golden hash chains are committed, and any movement in them is
   explained in the PR that moved them.
3. **Golden playbooks**: the stage's playbooks round-trip byte-identically, verify identically
   (same `report_hash`), and render identically.
4. **Headless scenario runs**: `gamectl scenario run` passes the stage's scenario files, with
   assertions on events *and* hashes, not just "it didn't crash".
5. **From S2 (destruction & combat) onward, the three nightly adversarial scenarios** — Rusher,
   Turtle, Hunter — run in CI against the Balanced built-in operator on a fixed seed set. Each has a
   written alarm band: an arm winning more than about 65% of seeds opens a tuning task. A stage does
   not close with an alarm band open and unacknowledged.
6. The stage's **named depth task** and its embedded gate are met, or its written fallback is taken
   deliberately (fallbacks are written *before* the stage starts — see spec section 16).
7. Docs updated in the same PR: this file if a rule changed, `docs/design/` if a decision changed.
8. The owner reviews the stage demo. That review, not CI, is what ends the stage.

The pre-gate ladder, for orientation: spikes (wk 3) → walking skeleton (18.5) → S1 economy (23.5) →
S2 destruction & combat (29.5) → S3 authoring (34.5) → **the fun gate and the public v0.1 (35.5)** →
S4 radio/licences/symmetric map (41.5) → S5 operators (45.5) → S6 editor & library (53) → S7 watch &
share (57.5) → hardening → **v1 (63.5)**.

## 11. What not to build in v1

If a task asks for one of these, stop and ask. They are deferred by decision, not by oversight, and
most of them have a reserved proto seam waiting — filling the seam early is the expensive mistake.

- **No script runtime or script language.** No QuickJS/`rquickjs`, Boa, Luau, Lua, `wasmi`, WASM
  operators, or a custom mini-language. Nothing user-written executes. (v1.1 runs scripts
  *outside* the game; the in-game runtime is later still.)
- **No MCP server**, no `rmcp`, no tool server of any kind.
- **No SDK and no published API.** The Seat Gateway stays internal; `llms.txt`, `llms-full.txt`, the
  agent guide and published schema docs ship with v1.1.
- **No AI or LLM seats**, no `gamectl connect`, no agent sandbox, no heartbeat loop, no watchdog
  hooks. Opponents are the built-in operator, full stop.
- **No manual control** of the commander or anything else. The playbook is the only input during a
  Push. (A manual-control round exists only as a gate fallback.)
- **No `set_flag`, `clear_flag`, `branch`, `repeat` or flag predicates in playbooks.** The v1
  vocabulary is exactly what the editor renders: route steps with guards, handlers, selectors,
  `on_death`, `fallback`, options. Their field numbers are reserved; Load *rejects* an
  out-of-vocabulary construct with a code and a JSON Pointer and never silently strips it.
- **No player Dispatches** or seat-to-seat text. v1 has one-way engine chatter only; no seat reads
  it.
- Also out: human-vs-human, teams (`team_id` reserved), more than 3 seats, internet play, dedicated
  servers, voting, the web spectator, the RL interface, flowing water, beacon capture, Citations,
  grid islands, batteries, reinforced wall/gate/shield block, timed weapons, operator personalities
  beyond Balanced, the director camera, the expert editor view (raw JSON, command palette) —
  selectors *do* ship in v1 — translation (English, one string table), the art pass, accelerated
  player-facing AI-only matches, and over-the-air attacks (interception, spoofing, jamming) with
  their counters.

## 12. Working conventions

- **Simpler is better.** Scope discipline is a design constraint: one developer directs the agents
  and reviews everything. A smaller diff that ends playable beats a larger one that ends "almost".
- **No artificial caps.** A limit exists only where it is technically necessary, and it gets a
  coherent in-world explanation. If you find yourself adding a cap for balance, that is a tuning
  question for the owner.
- **Don't invent rules.** If the spec does not state it, do not implement it and do not write it
  into a doc as if it were settled. Raise it, with the options and their downsides.
- **Tuning values are data**, versioned in the rules table and stamped into the rules hash — not
  constants sprinkled through the code. A tuning value whose row does not exist yet is a named
  constant carrying a `PLACEHOLDER` that names the owner and the stage; proposing the row is part of
  that stage, not of the task that first needed the value (the row is a `proto/**` change, §5).
- Prose and identifiers use British spelling where the design docs do (`licence` the noun,
  `behaviour`, `armour`). Diagnostic codes and user-facing strings are English and live in one
  string table.
- When you must guess, write `// PLACEHOLDER: <what, who decides, when>` and list it in your PR.
