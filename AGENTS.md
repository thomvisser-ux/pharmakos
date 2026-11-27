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
`protoc` 36, `buf` 1.73, `cargo-deny`, `reuse`, Godot 4.7.2 and its export templates — and
`cargo xtask ci` is green: sixteen steps, every one required. A missing input is a failure, and
only three skips remain, each
a statement about the platform or the caller (`screenshot` off Linux, `stage-client` on macOS,
`test-research` when `--package` leaves the sim out); a missing tool skips locally and fails under
`PHARMAKOS_REQUIRE_TOOLS=1`, which CI and the merge train's local check set (decisions-log item
116 (6)(g)); the `--require-tools` flag alone covers the steps' own tools, not a test's. The three-OS matrix runs on every pull request, and the `cross-OS determinism guard`
job byte-compares every leg's determinism chain, scenario chains, path hashes, mesher, mapgen
and vista geometry digests and verifier reports (every report required on all three, the at-budget
`budget_128` report by name; decisions-log item 218), so §10's "all three operating systems agree on the hash chains" is
checked on every pull request that changes more than prose and on `main`'s push (item
116 (6)(n); a prose-only pull request takes the fast path of §9, item 115 (4)). The four stack
spikes are closed, frozen at the `spike-end` tag and deleted from `main`;
their measured results and their lessons for the skeleton live under `docs/spikes/`, and reading the
spike code means `git worktree add ../pharmakos-spikes spike-end`. The walking skeleton is closed
(decisions-log item 126, 2026-09-29), S1 is closed (decisions-log items 139 and 143, 2026-10-16), S2 is
closed (decisions-log item 155, 2026-10-28), S3 is under way (decisions-log items 153 and 154 and `docs/design/s3-plan.md`), and every crate in §3 has code; mark any value
you had to guess with a `PLACEHOLDER` comment naming who fixes it and when.

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
| `crates/proto` — `pharmakos-proto` | Generated prost types for `gp.v1` (playbooks, templates) and `gp.api.v1` (gateway), the canonical JSON mapping with the one JSON grammar (`json::grammar`) that the codec, the direct reader and plan-core's JSONC reader go through, and the quoting door (`Quoted`, `Said`) through which proto, plan-core, the verifier and the gateway build every sentence that carries a player's text, and the voxel run-length codec (`chunk_rle`) the gateway's view feed encodes with and the client decodes with, and the transport's message bounds (`transport`: the largest request, `MAX_MESSAGE_BYTES`, the largest answer, `MAX_ANSWER_BYTES`, a server frame's header, `SERVER_FRAME_HEADER_BYTES`, the largest frame, `MAX_ANSWER_FRAME_BYTES`, and the longest request id, `MAX_ID_BYTES`) that the gateway holds itself to and the client sizes its buffers from. The single schema source. Licensed `MIT OR Apache-2.0`, unlike the rest. | `prost` |
| `crates/sim` — `pharmakos-sim` | The deterministic sim: 20 Hz fixed tick, integer maths, SoA tables, 32³ copy-on-write chunks, runner (Lull/Push/recap), playbook interpreter, mandates, programs, Quartermaster, power grid, combat, kill-credit counters, pathing/ETA, map generation, snapshot/restore, replay, per-tick xxh3 state hash. `fork` lives **here and nowhere else**, behind `feature = "research"`. | `pharmakos-proto`, and the determinism crates (`xxh3`, `imbl`, `postcard`/`rkyv`, pathfinding primitives) |
| `crates/plan-core` — `pharmakos-plan-core` | Playbook core: canonical form, JSONC round-trip (comments survive), JSON Patch (a dialect stricter than RFC 6902, whose inverse is an exact `undo` form), `render_plan` prose, the interface-time and `$`/`kW` arithmetic re-exported from the verifier, travel estimates through the `TravelEstimator` seam its caller binds, and `stack`, the stated stack (`PLAYER_TEXT_STACK`) of every thread of `gamectl` and the gateway that reads player text. In process with the gateway. | `pharmakos-proto`, `pharmakos-verifier`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/verifier` — `pharmakos-verifier` | Seal inspection: decode → structure → resolve → semantics (QUICK) → estimate → lint (FULL). Diagnostic catalogue, `report_hash`, and the rules table's door (`read_rules`: its limits, interface rates, prices and walking rows; the bound of an interface row against the sim's `i32` is the sim loader's, `RulesError::RowPastTheSim`, decisions-log item 169). Spec §5's interface-time arithmetic (`interface.rs`) and the `$`/`kW` projection (`projection.rs`), moved down from plan-core in S1 and re-exported by it, and the walking lower bound (`walking.rs`), new in S1 (decisions-log item 133). | `pharmakos-proto`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/operator` — `pharmakos-operator` | The built-in operator: templates + utility scoring, the safe playbook, the wizard's suggestions, Easy/Normal/Hard. An ordinary gateway client with no privileged reads: from T18 it reaches the match only through a call closure over `gp.api.v1` JSON-RPC, which the gateway binds to that seat's own in-process token (`serve::InProcessSeats`) and `gamectl host` adapts to `serve::Operators` — a `BuiltInSeat` plays a seat; an `Advisor` returns a human seat's safe playbook and suggestions, which the host files — and it reads the public rules text for its tuning rows (decisions-log item 111). Filing on a miss is the gateway's (`begin_push`) and executing a sealed playbook is the sim's. | The library: `pharmakos-proto` only. Its tests may add `pharmakos-gateway` as a dev-dependency (T18 needed none); an operator test that hosts a match lives in `crates/gamectl/tests` |
| `crates/gateway` — `pharmakos-gateway` | Seat Gateway: JSON-RPC over a localhost WebSocket, tokens, scopes, fog filter, rate limits, event bus, snapshots, private match cache, saves and resume, and the private replay's inputs. Hosts the match, and refuses at its start a rules table the verifier refuses. | `pharmakos-proto`, `pharmakos-plan-core`, `pharmakos-verifier`, `pharmakos-sim` *only* as `default-features = false` |
| `crates/gamectl` — `pharmakos-gamectl` (bin `gamectl`) | CLI: `verify`, `schema`, `docs`, `scenario run`, `seat doctor`, `host`, every command run on one thread of the stated player-text stack. No `connect` in v1. `scenario run` hosts a headless match and `host` serves one to the Godot client over its own stdio pipe (the config line in on stdin, the announce line with the port and the tokens out on stdout, exit when stdin ends), which is why the gateway is on the list. From T18, `host` links the built-in operator and adapts its client to the gateway's in-process seats (decisions-log item 111); from T18b, `scenario run` seats a `builtin` seat with Easy through `serve::InProcessSeats` (decisions-log item 113). | `pharmakos-proto`, `pharmakos-plan-core`, `pharmakos-verifier`, `pharmakos-gateway`, `pharmakos-operator`, and `pharmakos-sim` *only* as `default-features = false`, because the gateway's API is written in the sim's types. `gamectl` drives a match only through the gateway's `Host` and `Surface`, never through the sim's `Runner` (decisions-log item 109) |
| `crates/mesher` — `pharmakos-mesher` | The **walled** greedy mesher: integer chunk data and `.vox` models in, vertex and index buffers out, under the per-frame upload budget the client applies. Links without gdext, so the headless CPU proxy and the CI geometry check reuse it. | `dot_vox` later; nothing from the sim. Never depended on by `sim`, `plan-core`, `verifier`, `operator`, `gateway` or `gamectl` — `cargo xtask wall-guard` fails the build over it (`WALL_GUARDED_PACKAGES`, §4.9) |
| `crates/client-gdext` — `pharmakos-client-gdext` | **Thin** gdext bridge (`cdylib` + `rlib`): marshals gateway calls and mesh buffers between Godot 4.7 and Rust. It marshals; it does not decide, and rule 4 names the pieces that schedule or draw without deciding. | `godot` (gdext), `pharmakos-proto`, `pharmakos-mesher` |
| `crates/bench` — `pharmakos-bench` | The **walled** timing harness for P1 (decisions-log item 131): QUICK's p50 and p99 over the committed verifier cases and `budget_128`, and the wall-clock cost of a decision tick per size unit from a hosted match whose playbooks sit at the size budget; and, from S2's `g3h` and `g3r` (decisions-log items 149 and 151), G3′-real's 8-minute segment at the gate's load with its phase, repath, drift, combat, footing and support-search figures, and the power sweep run by hand with the certifying runs; and, from S3's `cert3` (decisions-log item 218), the commands S3's gates are certified on, each naming its commit and stamping every line with what it sampled: P1's certifying run (`p1_certify`, with the editor's end-to-end wait and E12's seed rule), the decision tick's cost per counted unit by kind (the covering kinds, near and far, among them), and G3′-real's watch (the view feed's bytes, each seat's peak sightings and the breach search at the gate's load). Published by `cargo xtask perf-alarms`, and the segment's tick read by `cargo xtask g3-budget`, and P1's and P3's report lines by `cargo xtask p1-budget` and `cargo xtask p3-budget`. Never shipped. It restates the verifier goldens' fixture, and `tests/harness.rs` holds the restatement to the committed reports. | `pharmakos-proto`, `pharmakos-verifier`, `pharmakos-gateway`, and `pharmakos-sim` *only* as `default-features = false`. It takes no `CLIENT_WALL` line, because it exists to drive the sim from outside; no deterministic crate may depend on it (`WALL_GUARDED_PACKAGES`, §4.9) |
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

1. **The research-feature ban.** `fork` exists only behind `pharmakos-sim`'s compile-time `research`
   feature, which is defined in that crate and in no other. Release builds never enable it; CI
   builds both configurations. `plan-core`, `verifier`, `operator`, `gateway`, `gamectl` and the
   walled `bench` must never reach it — not in `[dependencies]`, not in `[dev-dependencies]`, not
   through a default feature, not transitively. They declare no `[features]` section of their own,
   and any dependency they take on the sim reads
   `pharmakos-sim = { workspace = true, default-features = false }`. `cargo xtask ci` walks
   `cargo tree -e features` and fails the build if the feature reaches any of the six
   (`GUARDED_PACKAGES` in `xtask/src/main.rs`).
2. **No dry runs.** `plan-core` and `verifier` may estimate — travel over known terrain,
   interface-time arithmetic, `$`/`kW` projection, placement legality, selector previews, mast
   coverage. The verifier's travel is `walking.rs`'s lower bound (the admissible octile bound at the
   rules table's step costs, between places the view names); the pathfinder stays in the sim and the
   gateway, and plan-core reaches it only through the `TravelEstimator` seam its caller binds
   (decisions-log item 133). They may never step or fork the sim, run mandates, programs, combat or
   construction, model enemy behaviour, or evaluate rule conditions over a projected future. The
   verifier checks targeting's names and descriptions and **never ranks** them: "nearest" is the
   sim's and the gateway's, and `crates/verifier/tests/confinement.rs`'s `the_verifier_never_ranks`
   is the tripwire (decisions-log item 131). Prefer depending on the sim's snapshot and knowledge
   types only; if you find yourself wanting its stepping API, the design is wrong — stop and ask.
   The gateway, which hosts the match, is held to the same rule: its planning methods
   that work on a playbook's text (`verify_plan`, `render_plan`, `patch_plan`,
   `instantiate_template`, `resolve_refs`) answer from the frozen snapshot and the rules table —
   `resolve_refs` ranks "nearest" through the sim's own
   resolver (`targeting::cover`, `targeting::on_vent_counted`, the `Ranker`) in the match's one lent
   planning scratch (`routes::PlanningScratch`, kept by the `Host`, whose `lend` asks the sim's
   `Scratch::serves` on every loan), over `World::ground`, whose meter counts nothing (the world holds no decision tally: `dunit`
   part 2, decisions-log item 196; `get_map_summary`'s and `estimate_route`'s reads still lend a tally
   nothing reads, which item 196 (7) carries), over the hosted world (the frozen world in a Lull; whenever the phase is not
   a Lull — a Push, a recap, the lobby, an ended match — the dispatcher's planning door answers
   `PHASE_CLOSED`, as for every `plan`-scoped method, and has since T9; from `econ` #92 the handler
   holds its own gate in a Push as well, before it parses anything, as defence in depth,
   decisions-log items 133 and 135), a preview of what a step will bind and not a dry run;
   `set_value_text` and `new_rule` answer from the caller's text and the rules table alone (text in,
   text out through `patch_plan`'s own door; `set_value_text` also reads the frozen snapshot's own
   seat row, as `render_plan` does), and `compose_wall_box` reads the true ground and the beacon's
   sphere over the hosted world, in a Lull and for the caller's own beacon only, through the sim's
   own `site::ground_under_walls` and `site::check_wall`, a preview and not a dry run; each of the
   three holds its own phase gate before it parses anything, beside the dispatcher's planning door
   (decisions-log item 187); and
   `get_map_summary`'s features (unfogged in S1, decisions-log item 130 (3) (a)) read the hosted
   world through the same resolver, their `travel_ms` and `reachable` told to a seat in a Lull only and
   withheld, both absent, in every other phase and for a caller that is no seat, with no walk priced (the owner's "Omit them in a
   Push", decisions-log item 170); `estimate_route` answers `PHASE_CLOSED` in every phase but a
   Lull, before it parses anything (the owner's "Refuse in a Push", decisions-log item 169), and in a
   Lull prices each leg at the walk the router serves over the hosted world's own graph
   (`RouteAdapter::over`: a borrowed `Surface` and `Clusters` and the match's lent planning
   scratch, no `World`),
   `estimate_route`'s `covering` legs read through the same resolver, and the gateway's knowledge and
   view reads read the frozen snapshot or the live world through the fog filter, and none of them
   steps anything; `submit_plan`
   *compiles* the playbook (the sim's `Plan::compile`, called once, from `compile_decoded` in
   `surface/planning.rs`, which `compile_canonical` and `resolve_refs` share (a `#[cfg(test)]` helper,
   `compile_playbook`, decodes a text and calls `compile_canonical` for the gateway's own tests), and
   which says a `PlanError` in the gateway's own words: the type is named in
   `surface/plan_words.rs` and on that one compile site, and `tests/confinement.rs` holds it there),
   a pure function of the playbook and the rules table that steps nothing — compiling a playbook is
   not driving a match (decisions-log items 103 (9) and 104 (6)). The match itself is stepped and
   sealed in one module, the gateway's `host.rs`, which checks its rules table at its start: in
   `serve::run_in` before anything is written, and again in `Host::open` (`Host::check_rules`, the
   verifier's `read_rules`), so a table the verifier refuses hosts no match. No handler steps or
   seals, *except* the admin-scoped control handlers in
   `crates/gateway/src/surface/control.rs`, which drive only the live match through `Surface`'s
   driving methods and may name none of `Runner`, `Host`, `World`, `host_mut`, `seal_plans`,
   `seal_playbook`, `snapshot` or `.clone()`; `crates/gateway/tests/confinement.rs` is the guard
   (decisions-log item 107), and its
   `a_handler_may_compile_a_playbook_and_may_never_seal_or_step_one` holds the compile to one call
   in the handler modules. The in-process seats are host-side the same way:
   `Surface::register_in_process` and `Surface::file_advice` are reached only from `serve.rs`,
   through `serve::InProcessSeats`, and the same test's `no_handler_can_file_advice` is the guard
   (decisions-log item 111). So is the scenario harness's door, `Surface::seal_by_harness`, which seals a
   text the verifier refused: it is reached only from `gamectl`'s scenario runner, audited as
   `seal by harness` and held to the cap every way of sealing has, and the same file's
   `no_handler_can_seal_by_the_harness` is the guard (decisions-log item 195). So is the restore: `Host::resume` is the one restore, and `.restore(`
   stays in `host.rs` (the same test's `the_match_is_stepped_in_one_module`); the save seams —
   `Save::parse`, `Surface::resume`, `Surface::take_persistence` and `Surface::lull_save` — are
   reached only from `serve.rs`; beside them stand the last Push's capture, `Surface::last_push`
   (reached only from `saved_match`), and its restore, `Surface::restore_last_push` (reached only
   from `Surface::resume`), both in `crates/gateway/src/surface/last_push.rs`, and the `ended`
   boundary (`Boundary::Ended`), written by `Surface::persist_end`. Their guard is the same test's
   `no_handler_reads_the_save_or_the_replay`, with its helper `surface_calls_no_save_seam`: it keeps
   them out of every handler module and every dispatch arm, and exempts their definers (`cache.rs`,
   `save.rs`, `surface.rs`) and `host.rs` as well as `serve.rs` (decisions-log items 113 and 202).
   An ended match is not resumed: `Save::check` and `Surface::resume` refuse an `ended` save, which
   `gamectl host` says on standard error with exit code 2. A boundary is durable before anyone is
   told it happened: the host answers the call that crossed it only once what it left is written, in
   one order: the replay inputs (the sealed playbooks, the chain), then the audit lines, then the
   save, which is the boundary's commit record and is written only when everything before it
   landed, so a failure leaves the previous save, from which a resume rebuilds what was lost; a
   write that fails answers that call and every later one with a typed `INTERNAL` saying what could
   not be written and what the disk holds, read back from the cache, and the host stops (`gamectl
   host` exits with code 4) (decisions-log item 202).
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
   (decisions-log item 114); and, from S1's `fixc` (decisions-log item 129), `editor.gd`'s click
   counters, its closing of a map menu on a press on the panel and its focus release on a map
   click, `--log-input` logging in `editor.gd` and `lobby.gd`, `rows.gd`'s rebuild only when the
   rows change (otherwise it only sets its Fix buttons' enabled state), `lobby.gd`'s speed label
   shown only in a Push (a branch on the bridge's phase string, for display), and the watch strip
   and editor hidden until a match is announced; a Fix button kept across a redraw still calls
   `Editor::fix`, which refuses unless the rows are current and nothing is in flight; and, from S1's
   `ui` (decisions-log item 136), `rig.rs`'s keyframe back-off, the keep-alive sum, which
   `tests/no_arithmetic.rs` names word for word in `SCHEDULING_SUMS` (S1's plan, decision 17), the
   rig's choice of a Lull's length by round (`LullLengths::for_round`) and its reading of
   `_status.untimed` to hide the countdown, the recap read once per recap and drawn by `lobby.gd`
   only in a recap or after the match ends (a branch on the bridge's phase string, for display),
   with a refused read drawn as the gateway's refusal, `targeting.rs`'s spelling of `vent_<x>_<y>`
   back into a kind and a column for the panel's words, its lax read of whether a reference is a
   name or a description, `has_feature_refs`, which decides whether `resolve_refs` is asked at all
   (otherwise the panel says no step names a vent or a seam), and its find of the "This round:"
   sentence by the gateway's own lead words (`editor.gd` hides the heading when there is none), the
   editor's vent pick by screen distance over `get_map_summary`'s features on
   `ViewModel::ground_at`, and the E0601 checkbox: its removal patch, composed from a click,
   `DORMANT_CODES`, the client's reading of the diagnostic catalogue for when the checkbox shows,
   and `dormant_key`, a lax reader of the player's file for whether the option is set and under
   which spelling; and, from S1's `oper` (decisions-log item 136), the editor's and the rig's
   handling of `estimate_route`'s `NOT_FOUND`, whatever its cause (a `covering` that covers nothing,
   a selector that ranks nothing, an unknown or unseen beacon), drawn as the route's answer
   (unreachable, no polyline, the gateway's sentence) and not counted or reported as a refused call,
   while any other refusal of the estimate still is one; and, from S1's `fixd` (decisions-log items 139 (2) and 143 (2)),
   the editor's door (`Door`, `Editor::set_door`), which the rig sets from the phase it reads and this seat's Ready
   (`Rig::door`), and through which the editor refuses an ask outside an open Lull, drops what was queued and says why a
   check will not come (`Editor::unchecked`), with the gateway's refusal of a check shown on that check's own line
   (`Check`); `SealLine`, a branch on the door, the verifier's `qualifies` and the filing; the rig's read of the
   gateway's `safe_playbook_filed` feed kind (`SAFE_PLAYBOOK_FILED_KIND`, held to the gateway's source by a test); the
   text's file (`Editor::file`, `Editor::saved_to`), which `editor.gd`'s `text_file` reads and `open_save_as` offers
   (the text's own file, or no name), as Save does; the filing (`Editor::safe_playbook_filed`, `Editor::filed`), the
   gateway's words on the status line, the "This round" sentence dropped and the mark cleared at a new round; the door's gating of the Fix
   buttons' and the E0601 checkbox's enabled state; and `editor.gd`'s `map_open` (a branch on the bridge's phase
   string), `map_click` (the map's click routing) and `unchecked_text`; and, from S2's `rnds` (decisions-log item 146),
   `godot/scripts/host_link.gd`'s test-only `--rounds` reader: `ROUNDS_ARG`, `rounds_arg`, a pure function of the arguments it is
   given, which answers whether `--rounds` was given, its text and a `RoundsRefusal`; `round_limit`, which reads the
   launch's user arguments through it and answers the text, or `ROUND_LIMIT` when none was given, for `config_line`'s
   round-limit field, and neither parses nor bounds it; `RoundsRefusal` with `rounds_refusal`, which reads the launch's
   user arguments through `rounds_arg` too, the client's only refusals of the option, each about the command
   line's shape or the line's framing and never the number (`NO_VALUE`, `REPEATED`, `BREAKS_LINE`), made in `start()`
   before any host is spawned, every refusal of the value being the gateway's; and `watch_check.gd`'s `_rounds_cases`,
   which runs `rounds_arg` over a fixed table first and fails the check on a mismatch; and, from S2's `view` (decisions-log item 149), `targeting.rs`'s `attack_targets` (`AttackAim`, `AttackMark`, `aim_of`, `voxel_aim`, `SELECTOR_ARMS`), a lax reader of the player's file for each Attack beacon's target, read once per text, with a selector and the enemy commander given as words only, and `editor.rs`'s `lax_document`, the lax JSONC read the route, E0601 and targeting readers share; `editor.gd`'s `_draw_attack` and `_place_attack_marks`, which mark a voxel target and put a named beacon's mark over the vista's marker of the same id (`vista.gd`'s `marker_node`), hidden while the view shows none; `chip_line`'s branch on the gateway's `reachable`; `vista.gd`'s shell, wreck and ruin markers, its hit-point bar filled by `hp` over `max_hp` as sent (`_set_bar`, drawn only when `hp` is present), its turn to the view's heading, and `_same_look`'s redraw on a change of kind, subtype, owner or bar presence, and `bridge.rs`'s `yaw_of`, a unit conversion for drawing; `view.rs`'s `entity_of`, the client's refusals of a view entity, each about the answer's shape (no position, half its hit points, a zero maximum, hit points past the maximum, a heading past the turn), and `ViewModel::new`'s refusal of an extent past the sim's `i32` coordinates; `palette_of`'s S2 rows, a wall drawn by the seat its material names; `edited_box` and the mesher's `LightField::relight`, which choose what to relight and redraw from two copies of what the gateway sent; `rig.rs`'s `served_status`, `refusal_of`, `proto3_text`, `event_rows`' refusal of a negative time, the pacer's refusal of an advance past its ask, and `HostClock::unreportable` with the rig's one sentence for it, and `variant.rs`' `proto3_integer`; and `host_link.gd`'s `GAMECTL_ARG`, `ROOT_ARG`, `prefixed_arg` and `repeated_option`, about the command line's shape and never the value, refused in `start()` before any host spawns; and `watch_check.gd`'s `_option_cases`; and, from S3's `fixp` (decisions-log item 174 (8)), `value_field.gd`'s select on focus, the focus released at open, Enter and Send emitting the text as typed, and the first-click rule ending at the first key typed; a refusal drawn in the value field's popup (`FieldAsk`, `FieldAsk::Refused`, `status_rule_refused_changed`) and on the chip's rule (`RuleRefusal.key`); `Editor::field_opened`, `bridge.editor_field_opened()` and the token carried by `Job::RuleEdit` and `Job::Visit`, so only the field that asked hears the answer; `_draw_rules`' closing of the value field's popup, the choice menu, the name menu and the condition picker when the rendering changes, and `_rebind_number`; the one close after an action pick (`close_menu`, `_map_menus`); the lobby's `ready_line`, a branch on the watch state's `sealed_this_round` (sealed, not sealed, not told); the entry list with its wrapped lock sentences, its placement beside the parent and its `ScrollContainer` capped at the window's height (`open_entry_list`); the popups' one opaque panel (`opaque`); the Build target reader (`targeting::build_targets`) with its marks and box outline (`_draw_build`, `_box_outline`); the dormant box's refusal path (`DormantRefusal`, standing only while its reason does) and its logging (`dormant_log`, and `lobby.gd`'s `--log-input` line for it); Load's folder (`load_folder`, `last_file`) and the editor state's `build_targets` key; `values.gd`'s `int_of`; and, from S3's `prob` (decisions-log item 182 (8)), `lobby.gd`'s `new_match_text` and `preset_name`, the preset's name from `HostLink.PRESET` through `strings.gd`'s `preset_<name>` rows, deciding nothing, and `preset_line`, a branch on the bridge's `preset_check` word, for display; `host_link.gd`'s `reset`, after a refused Resume; `rig.rs`'s `StatusFacts`, `MatchPreset`, `ServedPreset` and `PresetCheck` and the seat's open-time `get_status` (scheduling), and `bridge.rs`'s `watch_expect_preset`; `wizard.rs`'s `TemplateRow.guided` and its unreadable-list refusals, `editor_view.rs`'s `guided`, `guided_reason` and `templates_read`, `editor.gd`'s mark and sentence drawn as sent, and the per-Lull re-ask of `list_templates` (scheduling); `rig.rs`'s `event_rows`, `EventRow` and `Speaker` (a branch on the typed payload) and `lobby.gd`'s `event_text_of`, drawing a memo under the speaker word; `rig.rs`'s `Recap.memos`, read once per recap and drawn by `lobby.gd`'s `recap_text_of` as sent; `view.rs`'s `Entity.ring` and its two refusals, `bridge.rs`'s `ring` key and `vista.gd`'s `_set_ring` and `_ring`, drawing the served radius; `bridge.rs`'s hostless `watch_recap_of_response`, `watch_rows_of_response`, `watch_status_of_response` and `editor_templates_of_response` (`wizard::templates_of_text`), for the shots; `editor.rs`'s `ReadFailed` for the template list (`templates_failed`, `templates_read`) and `templates_unchecked` (the shut door, in the checks' words, through `editor_view.rs`'s shared `unchecked_dictionary`), `bridge.rs`'s `status_failed`, `status_detail`, `sealed_this_round` (the rig's: the seat's word for this round and phase only) and `templates_known` keys, and `lobby.gd`'s `lobby_status_*` words; `rig.rs`'s `SealFact` and the seat's `get_status` at each Lull's opening and after an accepted seal (scheduling), `Editor::take_sealed_mark`, and Ready waiting behind that read; `lobby.gd`'s `_offer_chooser` after any failure (a branch on "a failure came, and was it a refused Resume", reading no words) and `_resuming` cleared on the announce, and `host_link.gd`'s `_connect` and `_read_announce` stopping once the link is done, with `reset` deferred; `editor.gd`'s static `fill_templates` and `guided_in` and `GUIDED_LOOK`, the mark a look with the gateway's sentence as its only words; `smoke_check.gd`'s lookup of New match by `Lobby.new_match_text()`, `watch_check.gd`'s Probation leg (the six-field line refused, `reset`, the lobby's line, `get_status` naming `probation`, the guided mark drawn as sent) and the custom legs' `get_status` check, and `client_check.gd`'s `_check_the_lobby_after_a_failure`; and the hostless scenes `ring_shot`, `lobby_shot` and `templates_shot` and their scripts, which draw committed byte copies and decide nothing; and, from S3's `fixq-c` (decisions-log item 197), the editor's planning wire and the transport under it, none of which decides a rule: `view.rs`'s `Entity.look` (`Look`, `LookShape`), `title` and `building` carried as sent, refused with the page when they do not fit the answer's shape (a look on anything but a structure or a ruin, a look with an edge of zero or a shape this build has no name for, a structure or a ruin with no look, going up on anything but a structure), `bridge.rs`'s `look`, `title` and `building` keys, and `vista.gd`'s `_look_mesh` (the primitive a look names within its voxels: `TOWER_SHARE`, `DISH_FOOT`, `DISH_HEIGHT`), its see-through drawing of a structure the view says is going up (`GOING_UP_ALPHA`, read from `building` alone), its name from `title` and `_same_look`'s redraw on a change of look, title or going up; `rule_list.rs`'s `RuleChip.outside` and `RuleChip.op` as sent (`op_of`), `chip_patch`, `remove_patch` and `op_patch`, `ChipEditor::Number`'s `display_unit` and `value_text`, and `ChipEditor::Choice`'s `titles`, refused when they do not pair with the values; `vocabulary.rs`'s `AnchorShape`, `queue_blueprints`, `groups`, `group_value`, `setting_parents` and `add_composed_target_row`, and `HeldTarget.line`, refused when the lines do not pair with the targets; `editor.rs`'s `set_number_text`, sending the text as typed to `set_value_text` (`Job::SetValue`; `Unchanged` when the text is the chip's `value_text`, a comparison of texts that reads no number), `change_setting_number_text`'s two calls (`Job::VisitValue`, `Job::VisitValueText`; the new step's index read by the lax `route_len` to name where the gateway writes, `RouteUnread` when it cannot be read) and `Undo::Text` (the text from before an edit made in two calls, restored with no call), `new_rule` (`Job::NewRule`) and `remove_rule`, `choose_group`, the wall's two-click pick (`PickOn::WallBox`, `pick_corner`, `Job::WallBox`; `NoAnchorShape`), `edit_of`, `slot_patch_of` and `voxel_params`; `editor_view.rs`'s `editor_into` (a setting row carries no `outside` and no `pick_op`) and its `anchor_shape`, `queue_blueprints`, `groups`, target `line` and `travel_text` keys; `targeting.rs`'s `ChipFeature.travel_text`; `bridge.rs`'s `editor_new_rule`, `editor_remove_rule` and `editor_choose_group`; `editor.gd`'s titles drawn in the choice and setting menus (`setting_value_text`, which says a value is unlisted or not shown and never draws its JSON), the value field pre-filled with `value_text` in `display_unit` and its bounds said in `unit`, joined to their unit with a space (a join of the gateway's words, not a conversion), the Remove list's lines, the Queue's own list (`ENTRY_LISTS`), `_on_rule_remove` and `_on_rule_add`, and the picker's dispatch on a group's or a predicate's metadata; `rule_list.gd`'s Remove and Add a rule buttons and `condition_picker.gd`'s groups section; the Ready line's facts, which are the gateway's: `rig.rs`'s `Rig::forget_seal` (an accepted submission, a new round and a reopened seat link each clear the seal fact, so the line says the client is asking), `status_read_failed` and `STATUS_RETRIES` (a refused read owed again only once `Timing::keepalive_due` says the seat link has been quiet for the keep-alive, `status_retry_owed`, reset at every phase change; scheduling), `Rig::seal_not_told` and `lobby.gd`'s `lobby_ready_not_told` words; the transport cap and buffers: `pharmakos_proto::transport::MAX_MESSAGE_BYTES`, which the rig checks every frame against as it is sent (the send path, after `frame_text` builds it) and refuses one past it before it is sent, and `transport.rs`'s `OUTBOUND_BUFFER_BYTES` and `INBOUND_BUFFER_BYTES` (`pharmakos_proto::transport::MAX_ANSWER_FRAME_BYTES`: the largest answer the gateway sends and a frame's header, decisions-log item 210), passed through by `bridge.rs`'s `watch_transport_limits` and set on every peer by `host_link.gd`'s `_connect` and `new_peer`, which compute neither, with `packets_of` reading a peer's packets and `pump` reading `send_text`'s error and handing it to `watch_send_failed` (`Rig::send_failed`, `Rig::send_failures`), and `strings.gd`'s `message_limit` and `Strings.text`'s fill of `{limit}`, which writes in the number as sent; the settling of a call that did not complete, which is not a decision about the game: `rig.rs`'s `Unfinished` and `Rig::unfinished`, the one place where a call refused, too large to send, failed to send, closed on, unreadable or timed out is settled per purpose, with `ClosedOnMessage`, `Rig::closed_on_message`, `Close`, `Rig::closed`, `Rig::link_open`, `Purpose::word` and `connection_word`, the `Ledger` (`late`, `idless`), `admin_reread` and `Recap::refusal_cause`, the held requests (`held_plans`, `plan_asking`, `Unfinished::Held` and `Reconnect::closed_free`, which holds an editor request that a link which has answered closed on, until the text changes or the next phase) and the held view (`held_view`, `view_asking`, `view_call`, `is_held`, `Rig::view_held`, asked again only after the keep-alive and only in a new phase), `editor.rs`'s `Cause` (`AnswerTooLarge`, `TimedOut`), `Editor::unfinished`, `UnfinishedCalls`, `UNREADABLE_KEY`, `lost_pending`, `lost_resolved` and `ReadyPress` with `Editor::set_ready_pressed` (a pressed Ready shuts the editor to new asks as the sent Ready does), `Editor::door`, which answers the door a pressed Ready shuts and which the panel's `door` key, the E0601 checkbox and `shut_to_asks` read, and `bridge.rs`'s `closed_on` with `lobby.gd`'s `status_line` and `closed_on_line`, a branch on it, for display; the reconnect policy, which is connection scheduling: `Reconnect`, `RECONNECTS`, `RECONNECT_BACKOFF_US`, `LinkAction`, `Rig::link_action`, `Rig::give_up` and `Rig::link_attempts`, with `bridge.rs`'s `watch_link_action`, `watch_give_up` and `watch_link_attempts`, `host_link.gd`'s handing of each close once to the rig and its `gave_up_words`, and, from S3's `outcap` (decisions-log item 210), a host that has exited: `Rig::host_exited`, told once with the host's last line, which makes `Rig::link_action` answer `GiveUp` for a closed link at once, and `GaveUp.host_exited` with the host's words, `bridge.rs`'s `watch_host_exited` and the `host_exited` and `host_words` keys of `watch_give_up`, and `gave_up_words`' host lead, so the lobby says the host's own words first and waits out no reconnect budget; the call deadline (`CALL_DEADLINE_US`, `Link.in_flight_since` and `close_owed`, `settle_overdue`, `Rig::link_must_close`, `Unfinished::TimedOut`), the resent Ready (`READY_RESENDS`, `ready_resends`, `ready_resend_at`, `Rig::ready_failed` with the bridge's `ready_failed` and `ready_failed_detail`, which `lobby.gd`'s `ready_line` reads for display), and the rig's decision to abort a link: `rig.rs`'s `Transport` (`of_ready_state`), `Link.transport` and `transport_since`, `Rig::transport`, `CONNECT_DEADLINE_US`, `CLOSE_GRACE_US` and `last_poll_us`, with `bridge.rs`'s `watch_transport` and `host_link.gd`'s `_pump_link` (it reports the peer's state every frame and aborts the peer when the rig says so, `close(-1)`, `_aborted`, reading an aborted link as closed), `_pump_links` and the test-only `attach` and `pump_attached`, which run the links against a listener with no host process for the watch check's silent-host legs, the sums of all of which (`waited_us`, `lasted_us`, the policy's and the Ready resend's) join `SCHEDULING_SUMS` word for word; the file write, which is not a rule (it replaces the file the player chose, or the lobby's one file, with the bytes it was handed, atomically, and says what is on disk, and no playbook can name a path, section 7): `write.rs`'s `write_whole`, `remove_whole`, `WriteFailure`, `Written`, `Removed`, `Files`, `RealFiles`, `temp_of`, `is_windows_reserved` and `has_windows_bad_ending` and its platform's refused names, `bridge.rs`'s `file_write`, `file_remove` and `file_failure_names`, `strings.gd`'s `write_words`, `editor.gd`'s `save_file` (its words `status_save_not_done` and `status_saved_with_note`) and `lobby.gd`'s `_remember`, `_forget` and `remembered` (its words `lobby_not_remembered` and `lobby_not_forgotten`, and `write_leftover` where a file could not be removed); `host_link.gd`'s `start` failing when the config line is not written, `instance_log.gd`'s said failures and `boot.gd`'s `_go`; and the checks: `src/rig/tests/faults.rs` (the seeded fault test, with its self-test of deliberately bad rigs), `tests/write.rs`, `tests/transport.rs`, `no_arithmetic.rs`'s conversion ban (`TYPED_SCRIPTS`, `CONVERSIONS`, `CONVERSIONS_KEPT`) and error scan (`ERROR_RETURNING`, `deferred_error`), `client_check.gd`'s `_check_the_looks`, `_check_the_planning_wire`, `_check_the_remembered_line`, `_check_the_transport` and `_check_a_close_on_a_message_is_said`, and `watch_check.gd`'s `_planning_wire`, `_pastes`, its closes-on-a-message and host-gone legs and its two silent-host legs; and, from S3's `prob` part 2 (decisions-log item 200), Playtest mode, which counts and exports and decides no rule: `pacer.rs`'s `MAX_ATTENTION_STEP_US`, `PlaytestClock` (`start`, `stamp_ms`, `seen`, `focus`, `push_began`, `push_ended`, `push_left`, `lull_began`, `lull_ready`, `lull_ended`, `lull_left`, `last_us`, `in_push`, `in_lull`) and `PushTime`, `LullTime`, `LullEnd` and `ClockFault`, together Playtest mode's wall time accumulated by focus and by phase with frame gaps apart, durations and never judgements; `playtest.rs` in full (the `Recorder` and its typed counters and records, `PushRecord`, `PushEnd`, `LullRecord`, `MatchRecord` and `MatchEnd`; `Code` with `of_refusal` and `of_verifier` and its closed sets; the in-memory diagnostics log and its closed grammar, `Event`; `DropReason`, the caps and `Recorded`; `Recorder::answered`, `Recorder::shown` and `Recorder::seen`, which read a rig answer's phase change and refusal code, the editor's error rows' codes and a frame's reading, and nothing else; `Recorder::match_begun`; `Survey::answered` and the survey's typed answers; `files`, `bundle` and the stored-zip writer; and `export`, which takes the survey with the path and writes the bundle through `write::write_whole` to the path the tester picked); `bridge.rs`'s `playtest` field with its one guarded call each in `watch_begin`, `watch_receive`, `watch_outbox` and `watch_command`, and `playtest_set`, `playtest_focus`, `playtest_camera`, `playtest_export` and `playtest_state`; `camera_rig.gd`'s `moved` signal, its `_panning` and `_turning` edges and their reset on a right press and on focus loss, a camera move said once when it starts; `playtest.gd` in full (`setup`, `switch`, `_notification`, `camera_moved`, `_process`, which makes no bridge call while Playtest mode is off, `show_state`, `note_text`, `_wanted_changed`, `_count_text`, `open_export`, `export_to` and `export_words`: no clock, no arithmetic and no file of its own); `lobby.gd`'s `_playtest_ready`, which places it; and the checks, `client_check.gd`'s `_check_the_playtest_lobby`, `_check_playtest_off_costs_nothing` with its `CountingBridge` and `_check_a_lost_release_counts_the_next_turn`, `watch_check.gd`'s `_playtest_on` and `_playtest_bundle` and the hostless `playtest_shot.gd`; and, from S3's `opsave` (decisions-log item 202), the player's side of a host that stopped: `host_link.gd`'s `_stderr_tail`, `last_line_of` and `_tell_host_exit` (the host's last standard-error line after its announce, told to the bridge once the host process has exited; the give-up stays the rig's), `lobby.gd`'s status-line display of the rig's `last_error` (`lobby_last_refusal`), `rig.rs`'s `last_error` rule (it holds the refused call's method, through the link's `in_flight_method`, and clears when a later call to that method is answered or the phase changes) and `watch_check.gd`'s `_ended_match_refused`, `_unsaved_host_said`, `_unsaved_host_lobby`, `_folder_named` and `_remove_tree` (test legs); and, from S3's `vcap` (decisions-log item 203), as S3's `leftout` left it (decisions-log item 214): `editor.rs`'s `LEFT_OUT_CODE`, the verifier's marker code restated, held to the verifier's source by `tests/report_cap.rs`, which also fails if the editor names a copy of the verifier's per-code cap again; `placement_verdict`, which draws a ghost not known (`GhostState::Unknown`) rather than legal when no error points into the placed step and the report's `left_out` holds an entry whose own `severity` is an error, with the report's own `I0007` words (`left_out_words`), and `Editor::conflicts_left_out`, which says, in those words, that conflicts were left out when an entry names the conflict code (`editor_view.rs`'s `conflicts_more`, drawn under the list by `rule_list.gd`); and `editor.gd`'s refusal to draw a ghost state it has no colour for (`ghost_unnamed`); and, from S3's `zerotitle` (decisions-log item 208), a zero's words and a bound's text, drawn as sent and never tested or converted: `rule_list.rs`'s `bound_text` and `untitled` (a bound sent without its text, text sent for a bound that is not set, a value title that is not its shape's `zero_title` and one on a choice or a reference, each refused on the answer's shape), `ChipEditor::Number`'s `min_text`, `max_text`, `zero_title` and `value_title`, and `ParameterShape::Number`'s `min_text` and `max_text`, carried as sent, `editor_view.rs`'s `bounds_into` and the `zero_title` and `value_title` keys of `editor_into`, `vocabulary.rs`'s `beacon_detail_of` copying a row's `value_title`, `editor.gd`'s `setting_value_text` (the title beside the number) and `_open_number` (`min_text` and `max_text` as the bounds, `value_title` as the value's words, and `zero_title` as "0 here: ..." when `value_title` is empty), `condition_picker.gd`'s `parameter_text` and `strings.gd`'s `rule_number_titled`, `rule_number_titled_beside`, `menu_setting_titled` and `rule_number_zero`; from S3's `outcap` (decisions-log item 210), a view listing that spans pages: `view.rs`'s `ViewModel::begin_listing`, which drops the entities joined from a listing that did not complete, and `rig.rs`'s `Answer::view_starts`, whether a page begins a listing (a keyframe, or a page asked with any cursor but the one the last unfinished page handed back; scheduling, because the cursor is opaque to the model); and, from S3's `edlists` (decisions-log item 211), a beacon's lists drawn and emptied: `vocabulary.rs`'s `HeldList` and `BeaconDetail.lists`, read as sent, with its refusal of a list with no clear pointer, and `clear_row`, which composes one `set_mandate_settings` row writing the sent `clear_pointer` `true`; `Editor::clear_list`, which composes the clear only for a row that carried the gateway's `clear_title` (its one refusal of shape is `RuleEditRefusal::NoClearOffered`); `bridge.rs`'s `editor_visit_row` `clear_list` row; `editor_view.rs`'s `lists` (title, lines, `clear_title`, `empty_title`, each as sent); `editor.gd`'s `_list_submenu` and `LIST_CLEAR_ID`, which draw the lines, the empty word when sent and the clear when sent and test nothing about the list; and `strings.gd`'s `menu_list*` keys and `status_rule_refused_no_clear_offered`; and, from S3's `leftout` (decisions-log item 214), what a report and a template list left out, read as sent from the typed fields `VerifyReport.left_out` and `ListTemplatesResponse.refused`: `editor.rs`'s `left_out_of`, which reads a report's `left_out` field and counts no rows, and the editor's `left_out` kept beside its rows (`Editor`, `CheckedPreview`), from which the ghost's Unknown verdict is decided by an entry's own `severity` and `conflicts_left_out` by an entry's code; `with_known_severities`, the client's refusal of a report whose `left_out` entry names no known severity; `wizard.rs`'s `TemplateList`, `RefusedRow` and `RefusedReason` with `refused_of`, the client's refusals of a refused-files entry with no known reason or a count of none; `editor_view.rs`'s `library_refused` state (`library_refused_array`); `editor.gd`'s `fill_templates` refused lines, `refused_lines` (a missing or malformed list drawn as the bridge's refusal), `refused_in`, `refused_drawn` and `LIBRARY_REFUSED_REASONS`, each drawing what the gateway sent; and the checks `client_check.gd`'s `_check_the_refused_library_files` and `watch_check.gd`'s `_refused_files_said` with `_refusing_root`. The client may compose a JSON Patch from a click; a verdict, a
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

The gateway reads no clock either, so the host must give it ticks in every phase the sim does not
advance — a Lull, a recap, an ended match: the client reports its clock through the admin-scoped
`report_host_clock`, and `gamectl scenario run` and `gamectl seat doctor`, which host a match with
no client and spend no host time in a Lull, pass each Lull's whole length to
`Surface::set_phase_remaining_ms`, a host-side call no seat can reach. Without that, the rate
limits, the audit log's stamps and token expiry, which count in the gateway's ticks, would stand
still for the whole phase; a reported clock that runs backwards or jumps too far is refused, never
clamped (decisions-log items 99, 103 (8) and (9), and 107 (3)).

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
golden it moves and names the rule that moved them (decisions-log item 114). A column added to a
table may be hashed as a block after the table's rows rather than inside each row, so a table that
holds no row hashes the same bytes as before and a chain that never fills it does not move; the
three places above still apply, and the PR still names every chain that moves (decisions-log item
136, the target table's order, rotation and footprint blocks).

For the chunk store there is a **fourth** place: a voxel write must *mark its chunk* so that
`settle` refreshes the chunk's digest. Miss the mark and the bytes change while the digest does
not — the hash is stale rather than missing, no golden moves, and nothing goes red until a replay
disagrees. Only `set` and `crater` may reach a chunk after generation, and both mark; a new write
path joins that pair and never bypasses it.

A table **derived** from the match's inputs is the exception that proves the rule: the feature table
(vents and seams) is a pure function of the seed, the rules and the occupied seats, so it is not
hashed per tick and not carried in the snapshot. It is generated with the map, regenerated by
`Snapshot::restore_into`, and pinned by its own golden
(`tests/golden/mapgen/expected.features.txt`). What it derives from the world, a feature's liveness,
is read from voxels the chunk digests already hash. A derived table that starts to hold anything a
tick writes is state, and goes into the three places above (decisions-log item 131).

A vector the world owns that a tick fills is a `Reserved<T>` (`crates/sim/src/reserved.rs`), whose
clone keeps the original's room and which a restore gives its room again
(`World::reserve_table_rooms`); `crates/sim/tests/allocations.rs`'s source guard,
`every_vector_the_world_owns_keeps_its_room_or_says_why_not`, walks every type the world owns and
fails on a plain `Vec`, `VecDeque`, `String` or `Box<[T]>` it is not told the bound of, and on a
`Reserved` field whose room no impl of its own type states (a struct literal's `with_capacity`, a
`vec![v; n]` buffer, or `reserve` or `reserve_total` on the field;
`the_room_check_finds_every_reserved_vector_given_no_room` is its must-trip test). Capacity is
never state: nothing in the sim branches on it (decisions-log item 201). A list a commit can grow has
its room stated in the source and a typed refusal past it: the Defend protect list's
`protect_room_of`; each seat's own list-settings and Attack avoid-area rooms, `seat_list_room_of` and
`seat_avoid_room_of` (the seat's beacon rows times `TARGETS_PER_BEACON` and `AVOID_AREAS_PER_BEACON`),
so one seat's lists never fill another's, a commit past its own seat's room writing nothing and
failing `targets_unwritten`, and `crates/sim/tests/list_rooms.rs` holding each room, and the
interpreter's `MAX_ROUTE_STEPS`, to at least the committed size budget, and `MAX_HANDLERS` to at
least half the budget (the working handlers a budget can hold); and planted
charges, one a beacon row (`PlantRefused::Full`, the carrier keeping its charge). The sighting table's
room is every asset the world's tables can hold alive at once (`sighting_room_of`): a seat forgets
its stalest sighting for room, by age alone, and the world counts what each seat forgets, a count
that is never hashed or saved. A restore refuses a save with a seat over its beacon share, its list
room, its avoid-area room or its sighting room, or holding a charge no run plants
(`SnapshotError::OverBeaconShare`, `OverListRoom`, `OverAvoidRoom`, `OverSightingRoom`,
`ChargeOutOfPlay`, `ChargesOverRoom`; no format changed; decisions-log items 209 and 219).

### 4.9 The wall is a crate boundary

Floats and wall-clock time are permitted only in the walled presentation/solve layer — rendering,
meshing, camera, UI easing, the Lull timer, and any offline solver whose output is converted to
integers before it is used.

**The wall is a crate boundary, not an `#[allow]` sprinkled through the tree.** Clippy has no
per-path allow-list, so the mechanism is this and nothing else:

- The walled crates are named in one place: `WALLED_PACKAGES` in `xtask/src/main.rs` (today:
  `presentation`, `solve`, `client-gdext`, `mesher`, `bench`; the last three exist). `clippy.toml`'s
  header repeats the list for readers.
- `cargo xtask clippy` pass 1 lints the whole workspace *minus* those crates with the full deny set;
  pass 2 lints those crates with `-D warnings` plus the float, cast, hash-map and clock allowances,
  **passed on the command line**. Nothing in the source asks for the allowance. The wall lifts
  exactly the lints in `WALL_ALLOW` (`xtask/src/main.rs`) and nothing else: the panic lints
  (`indexing_slicing`, `unwrap_used`) and the rounding lint (`integer_division`) stay denied inside a
  walled crate too, so a walled crate still reads slices through `get` and divides through
  `checked_div`. A scoped run (`cargo xtask clippy -p <crate>`, which the versioned pre-commit hook
  makes for each crate a commit touches) lints a walled crate named outright in pass 2, with the
  wall's allowances, and any other value, a glob or a package-id URL included, in pass 1; the
  research pass runs only unscoped (decisions-log item 136).
- `cargo xtask wall-guard` then fails the build if `sim`, `plan-core`, `verifier`, `operator`,
  `gateway` or `gamectl` depends on a walled crate, transitively included. That list is
  `WALL_GUARDED_PACKAGES` in `xtask/src/main.rs` — the research guard's crates less the walled
  `bench`, plus the sim, which cannot join the research guard because the sim is the crate that
  *defines* the `research` feature. The sim is on the wall's list because it is the crate that owns
  hashed state, so it is the one the wall exists to protect. That dependency edge is what makes the
  allowance safe, and it is only visible to CI because the wall is a crate.
- `wall-guard`'s second relation keeps the client side of the wall away from the core: `CLIENT_WALL`
  in `xtask/src/main.rs` pairs a walled crate, line by line, with what it may never reach,
  transitively included (`client-gdext`: the sim, the verifier, plan-core, the gateway and the
  operator, the crate of gameplay decisions, on the owner's decision of decisions-log item 118 (5);
  `mesher`: the sim), and fails too when a crate a line names has left the workspace.
  `crates/client-gdext/tests/no_sim.rs` checks the client's line from `Cargo.lock` as well. A walled
  crate on the client side of the wall joins by adding its own line; `bench` has none, because it is
  the walled harness §4.5 sanctions and exists to drive the sim from outside. The rule is not "no
  walled crate reaches the sim", which would forbid that harness (decisions-log items 102 (3), 116
  (6)(d) and 131).
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
  `overflow-checks = true` (`[profile.xtask]` among them, which the `xtask` and `ci` aliases build
  xtask in)
- The repository's cargo config, `.cargo/config.toml`: the `xtask` and `ci` aliases (each carries
  `--profile xtask`), the `[build]` wrapper keys set empty, and the per-host-triple `linker`
  tables. The tables and the wrapper keys are held equal to xtask's own (`DEFAULT_LINKERS` and
  `PINNED_TOOLS` in `xtask/src/resolved.rs`) by tests that read the file strictly, and
  `rust-toolchain.toml` is read the same way (decisions-log item 165)
- Determinism code: the state hash, the plan fingerprint (`pharmakos_sim::hash`, which a save
  stamps beside every seal; decisions-log item 136), the RNG stream enum, the fixed-point types,
  the snapshot and replay formats, the tick loop, the `research` feature and the `fork`
  implementation
- The formats a match leaves on a player's disk: the save container (`save.json`,
  `crates/gateway/src/save.rs`), at format version 4: a version-3 save still loads, without the last
  Push (`last_push`, decisions-log item 177 (5)), and its third boundary, `ended`, is written when a
  match ends and never resumed (item 202); the private replay's inputs — `match.json` (the seed, the settings
  and the rules hash, written by `crates/gateway/src/cache.rs`),
  `seats/<seat>/sealed/<round>.jsonc`, and `replay/<round>.hashes.txt` in the scenario hash-file
  format, which therefore lives on players' disks as well as under `tests/golden`; the audit log's
  leading sequence and tick fields, which a resume reads back (`MatchCache::continuation`); and,
  from T19 PR 2, the config line the lobby remembers in `user://last_match.txt`: the seven
  `serve::Config` fields that `serve::ConfigLine` reads (the seventh is the preset), without
  `resume`, and no token
  (decisions-log items 113 and 114; the file is replaced atomically from a temporary file beside it,
  `.last_match.txt.<pid>.<count>.pharmakos-write`, while it is written, and its format is
  unchanged: item 197)
- The Playtest bundle's format (`crates/client-gdext/src/playtest.rs`: `BUNDLE_FORMAT`,
  `BUNDLE_ENTRIES`, the keys and units of `manifest.json`, `survey.json`, `metrics.json` and
  `diagnostics.log`'s line grammar), which changes additively only; a reader of a later
  `bundle_format` skips a key it does not know and refuses a format it does not know (decisions-log
  item 200)
- The event vocabulary: an event kind's id and name (`EventKind`, `crates/sim/src/events.rs`), a
  step failure's reason id and name (`StepFailure`, `crates/sim/src/interpreter/state.rs`) and an
  event value's bit layout (`StepFailed`, beside it) are read by the interpreter transcripts, the
  scenario files' event assertions and event logs, and the gateway's feed, recap and strings, so
  each changes additively only: a new kind or reason takes a new id at the end and a new name, a
  shipped name never changes, a retired id is never reused (`not_own`'s 10), and a new field in a
  value takes bits every older value holds zero, so a value written before still decodes as itself
  (decisions-log item 135)
- CI: `.github/workflows/**`, `xtask/**`'s definition of what `ci` runs and of what
  `cargo xtask package` checks (`xtask/src/package.rs`, `xtask/src/zip.rs`), golden-file *formats*
  (including `tests/golden/package/`, the zips' manifests)
- Licensing: `LICENSES/**`, per-directory `LICENSE` files, the REUSE manifest
- Packaging: `packaging/REUSE.toml.in`, the zips' own REUSE manifest, and the licence texts
  `packaging/LICENSE-MPL-2.0.txt` and `packaging/LICENSE-BSL-1.0.txt`, which `cargo xtask package`
  copies into each zip's `LICENSES/`; `[profile.release-client]` and
  `.github/workflows/release.yml` are contract paths already, as a `[profile.*]` block and a
  workflow (decisions-log items 117 and 121)
- `AGENTS.md`, `CLAUDE.md`, and the design docs under `docs/design/`

**The rule for an agent:** if your change touches one of these paths, make the change on a branch,
open a PR describing exactly what the contract change is and why, and **stop**. Do not merge, do not
self-approve, do not work around the file (a `#[allow]` that defeats a determinism lint, a shadow
copy of a `.proto`, a second hash function) — a workaround is a contract change wearing a disguise.
Regenerating goldens counts: say in the PR *why* the hashes moved, and never regenerate them to make
a red test go green without explaining the behaviour change that moved them.

Proto rules that hold regardless: Protobuf is the single source of truth; fields are added, never
renumbered or reused; reserved field numbers stay reserved; `buf breaking` runs in CI in
`WIRE_JSON` mode; submitted playbooks with unknown fields are rejected. A change that discharges a
held field number, filling a reserved range with the field it was held for, adds an `ignore_only`
block to `proto/buf.yaml` scoped to the files it discharges in, with each discharge written out,
and the next proto change that discharges nothing deletes it, so an un-reservation nobody intended
never passes unseen (decisions-log items 100 (10), 109, 111, 129 and 130).

## 6. Parallel agents

Review is the bottleneck, so parallelism is capped by what can be reviewed, not by what the
machine can run: under the owner's delegation (CLAUDE.md) the main session reads every pull
request and the owner reviews the stage demo.

- **At most four agents at once** (the owner's word of 2026-11-25, "lower the max lane count to 4",
  the seventh since 2026-11-24, after "lower the max lane count to 6", "lower the max lane count to
  4", "add a 5th available lane", "add a 6th lane", "reduce max lanes to 5" and "add a 6th lane"; it
  supersedes 2026-11-23's seven, "raise the lane count to 7", through them; decisions-log items 183,
  184, 191 and 199). If you were spawned as a fifth, say so and wait.
- **Each agent works in its own git worktree**, never in the owner's checkout:

  ```sh
  git worktree add ../pharmakos-<task> -b feat/<crate>-<task>
  # work, commit, push, open a PR from that branch
  git worktree remove ../pharmakos-<task>     # when the PR is merged
  ```

- **One crate, one agent.** Two agents never edit the same crate at the same time. If your task
  needs a change in a crate another agent owns right now, stop and ask for the change to be
  sequenced — do not "just add a small helper" over there. By the owner's word of 2026-11-23
  (decisions-log item 183), for the rest of S3 two lanes may work in one crate at the same time
  when their briefs name disjoint files. Under that word, each lane's brief lists its files, a file
  both need is sequenced as above, merges stay one at a time, and the later lane takes `main` by a
  signed merge and re-runs the full check. A task granted a *named place* in a
  crate it does not own (a module, or a few named lines) is granted that crate's confinement-test
  line with it wherever the test lists the crate's sources by name, and the grant says so:
  `every_module_is_checked` in `crates/gamectl/tests/confinement.rs` fails on any `mod name;` that a
  listed file declares, `pub` or not, at any depth, when its file is missing from `SOURCES`; on a
  `.rs` file under `src/` that is missing from `SOURCES`; and on a listed file, `lib.rs` and
  `main.rs` apart, that no listed file declares; its twin in
  `crates/operator/tests/confinement.rs` still reads only the `mod` lines of `src/lib.rs`
  (decisions-log items 110 (4) and 185). A sweep (the no-silent-fallback rule, say) that
  reaches callers outside the lane's own crate or named place stops at that edge and says so in
  the PR; the next owner of the callers' crate carries the rest, written into that lane's section
  of the stage plan (decisions-log item 135). The one exception is a comment-only sweep: a sweep the
  stage plan needs finished across crates at once, so that a check can then hold every file to it
  (S1's PLACEHOLDER grammar, which the `placeholders` step parses, raised this), may be granted in
  that lane's section of the plan. The lane may then edit comments, and nothing else, in the crates
  and paths the grant lists. A comment in a contract path (§5) is still a contract change that the
  PR names; a `.proto` comment edit regenerates the prost tree with `buf generate`, and only its doc
  comments may move. The grant never covers SPDX or REUSE lines, the reason on an `#[allow]`, or a
  JSONC file (its comments are data). A marker sweep keeps each marker's what, who and when, and
  adds or removes none unless the grant names the change, and the PR shows `cargo xtask
  placeholders` before and after. A crate a running lane owns (one whose PR has not merged) is left
  out, and that lane's brief carries the sweep for it; the check that holds every file to the sweep
  turns strict only once both have merged. The PR lists every file it touched outside its own crate,
  and the main session checks at the merge that only comments changed there, reading by hand any
  line that `git diff -U0` shows outside a whole-line comment (decisions-log items 137 and 141).
  Tests that pin another crate's output are granted as
  named places, like a confinement-test line, in a crate no running lane owns: a lane that changes
  the operator's behaviour or `library/` is granted, in its section of the stage plan, the tests and
  goldens that pin that output — the operator's hosted tests in `crates/gamectl/tests`
  (`operator.rs`, `operator_host.rs`, `wizard.rs`; §3 rule 3), `crates/gateway/tests/advice.rs`, the
  client's wizard pins in `crates/client-gdext/tests/wizard.rs`, the gateway's
  `instantiate_suggested` golden with its byte copy `godot/fixtures/instantiate_suggested.json`,
  and the nightly result golden `tests/golden/scenarios/nightly-result/`, which Easy's output and
  the rules hash both move (decisions-log item 145 (7)). A
  crate another running lane owns is not granted this way: the change is sequenced, as this item's
  first rule says. An edit to an unowned crate that a lane finds it needs and was not granted is
  disclosed in its PR, and the main session rules on it at the merge (decisions-log item 136).
- **Take `main` only by a signed merge, never by a rebase.** A branch brings in `main` with
  `git fetch origin main`, then `git merge --no-ff --no-commit FETCH_HEAD`, then `git commit -s`
  (a `Co-Authored-By:` trailer, where there is one, above the sign-off, §8), and pushes the merge
  commit with a plain push. A pushed branch is never rebased and never force-pushed, and a merge
  conflict is resolved inside that signed merge commit. The desktop app's Auto-fix needs a
  branch whose pushed history is never rewritten, and the DCO walk checks merge commits as it
  checks every other. A pull request merges into `main` by a merge commit, pinned to the head
  whose checks passed, never by a rebase or a squash; `scripts/merge-train.sh` does both halves
  (decisions-log item 145 (3)). Never force-push `main`. The remedy for an unsigned commit, which
  never rewrites a pushed branch, is in §8.
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
- **A player's text is quoted wherever a person is shown it.** Where a person is shown a sentence
  built by `proto`, `plan-core`, the verifier or the gateway, a player's text in it is quoted:
  `Quoted` is the text escaped and capped, and `Said` is built from a `&'static str`, a number, a
  `char`, a `Quoted`, another `Said`, or the words of an `EngineWords` type (escaped and capped;
  implementable only inside these four crates, which
  `engine_words_are_named_only_behind_the_door` in `crates/plan-core/tests/door.rs` holds), never
  from a `String`. The rest of that file's scans hold the other back ways in. `gamectl` escapes
  its leaves where it prints them (`crates/gamectl/tests/shown.rs`), and the hidden-character
  table, `json::grammar::HIDDEN`, is data. Quoting changes what a person is shown and never the
  bytes of the player's file. So: do not build such a sentence from a `String`, do not add a way to
  make a `Said` or a `Quoted` from raw text, and do not weaken the scans to pass a change
  (decisions-log items 167 (5) and 168).
- **Every thread that reads a player's text has a stated stack.** Playbook recursion is bounded by
  the grammar's depth, and that is a bound on the stack only when the stack is a number: every
  thread of `gamectl` and of the gateway that reads player text is made by
  `pharmakos_plan_core::stack::player_text_thread` on `PLAYER_TEXT_STACK`, and the confinement
  tests (`crates/gateway/tests/confinement.rs`, `crates/gamectl/tests/confinement.rs`) each refuse
  any whole-word `thread` in their crate's shipped code, outside strings, characters and comments,
  other than `thread::JoinHandle`; both crates' tests take as shipped code every item but those a
  real `#[cfg(test)]` attribute stands on (in the gateway's, only the item's own characters, so code
  after its closing brace on that line is read) (decisions-log items 178 (10), 183 and 202). So: make every new thread in
  `gamectl` and the gateway through `player_text_thread` (decisions-log items 167 (6) and 168).
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
  removal of the mechanism. The owner's one exception, for the walking skeleton, S1, S2 and S3 only,
  is in CLAUDE.md: bypass mode in the owner's own session and the agents it launches, never against
  a real seat (decisions-log items 86, 116 (9), 126, 141 and 152).
- Rate limits, an audit log and message bounds on the gateway are part of the feature, not a later hardening task.
  A request is within `MAX_MESSAGE_BYTES`, and a message past it closes the connection with close code 1009; its `id` is within `MAX_ID_BYTES`, and one past it is an Invalid Request answered under no id. Neither is `RESPONSE_TOO_LARGE`, which is only for an answer. An answer is within
  `MAX_ANSWER_BYTES` or is the typed refusal `RESPONSE_TOO_LARGE`, sent in its place and never a cut text, with the
  call's effect standing and the connection kept: `Surface::call` holds the cap for every door (socket, host thread and
  in-process seat), and the audit log keeps the call's true outcome and adds a second line, `withhold answer
  <method>`, for the withheld answer. The cap is set by a rule and not a figure: the worst in-budget answer found is
  at most three quarters of it, held by `an_in_budget_playbook_at_the_inbound_cap_answers_whole` (decisions-log items
  207 and 210).

## 8. Commits

- **Conventional commits.** `type(scope): summary`, imperative, lower case, no trailing period.
  Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
  Scope is the crate or area: `feat(sim):`, `fix(verifier):`, `docs(design):`, `ci(determinism):`.
  A breaking contract change carries `!` and a `BREAKING CHANGE:` footer — and still needs §5
  approval. A merge commit is exempt from the subject rule: it reads
  `Merge main (<sha>) into <branch>` (§6), or GitHub's own subject for a pull request's merge.
- **DCO sign-off on every commit.** `git commit -s` adds
  `Signed-off-by: Name <email>`; keep it as the last line. The `dco` job in
  `.github/workflows/ci.yml` walks every commit in the pull request and fails without it. No
  sign-off, no merge. Dependabot's own commits carry its sign-off line,
  `dependabot[bot] <support@github.com>`, which is accepted for them in place of a person's
  (decisions-log items 119 (4), 120 and 122 (4)); a commit anyone else adds to its branch still
  needs its own. An unsigned commit that has not been pushed is fixed in place: `git commit --amend
  -s` for the last commit, or `git rebase --signoff <base>` for several. Once it has been pushed,
  the branch is not rewritten (§6): the commits are made again, signed, on a new branch, which
  opens a new pull request in place of the old one. The one force-push item 145 (3) records was
  the main session's, of a merge commit no review had read, and it is not a lane's remedy.
- **SPDX headers on every file**, REUSE-style, with `REUSE.toml` as the manifest and a `LICENSE`
  pointer at the root, in each top-level directory outside the game code's tier other than
  `LICENSES/` (`assets/`, `docs/`, `examples/`, `library/`, `packaging/`, `proto/`, `rules/`), and
  in `tests/`, whose pointer names its permissive golden areas; the game tier's other directories
  have none (decisions-log items 119 (6) and 121):

  <!-- REUSE-IgnoreStart: an example header, not this file's own -->
  ```rust
  // SPDX-FileCopyrightText: 2026 Pharmakos contributors
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  <!-- REUSE-IgnoreEnd -->

  Which licence a file gets depends on where it lives:

  | Directory | Licence |
  |---|---|
  | game code — sim, client, operator, gateway, gamectl, xtask | `GPL-3.0-or-later` |
  | `proto/`, generated JSON Schema, `docs/`, `llms.txt`, example playbooks, the template library `library/`, `packaging/` (the zips' note and manifest templates; its two licence texts keep their own terms), `CHANGELOG.md` | `MIT OR Apache-2.0` |
  | art and audio assets | `CC-BY-SA-4.0` |

  A file outside `library/` that reproduces a library template's expression carries the library's
  licence beside its own; `docs/LICENSING.md` ("A file that quotes `library/`") is the standing
  statement of the rule, and the `reuse` step checks it (§9 item 9).

  Third-party assets keep their own SPDX line and their entry in the credits screen (CC-BY
  attribution is a shipping requirement, not a nicety). Audio comes from CC0/CC-BY sources only.
- **The versioned hooks.** `scripts/githooks/` holds them, wired once per clone with
  `git config core.hooksPath scripts/githooks` (README, "Setting up a clone"): `pre-commit` runs
  rustfmt on the files a commit stages and clippy on the crates it touches, and `pre-push` runs
  `cargo xtask fmt clippy` over the workspace and refuses a push of anything but HEAD's commit.
  `PHARMAKOS_SKIP_HOOKS=1` and `--no-verify` are for genuine emergencies, never a way past a failing
  check (decisions-log item 132). The pre-commit rustfmt check leaves out the generated prost tree
  (`crates/proto/src/generated/`): `crates/proto/src/lib.rs` reaches it through `include!`, so
  `cargo fmt` never formats it, and `crates/proto/tests/generated.rs` holds it byte for byte to what
  `buf generate` writes; clippy still lints its crate (decisions-log items 137 and 141).
- An agent-written commit may add a `Co-Authored-By:` trailer above the sign-off. The sign-off
  itself must be a real identity that can make the DCO's assertion.

## 9. `cargo xtask ci`

One command for the suite. If it is green locally, CI's `cargo xtask ci` legs are green; if they are
not, that is a bug in `xtask`, not a reason to run something else. CI's required package jobs run a
second command, `cargo xtask package` (below), which `ci` never runs.

```sh
cargo xtask ci            # everything below
cargo xtask ci --quick    # fmt, clippy, unit tests — the inner loop
cargo xtask ci --fix      # rustfmt and the machine-applicable clippy fixes
cargo xtask ci --shard 2/3   # one of CI's shards (below); never needed locally
```

**The shards.** In CI each operating system's leg runs as `CI_SHARDS` (three) parallel jobs, `cargo
xtask ci shard <n> (<os>)`, each running `cargo xtask ci --shard <n>/3`, so that a leg stays inside
the owner's 75-minute budget (decisions-log items 119 (5) and 148 (2)). A shard runs its part of the
three slow steps, `test`, `test-research` and `scenario`, and shard 1 runs every other step as well,
except `golden`, which reads every shard's fresh outputs. The split is by name and reads no timing
table (xtask's tests hold the CI timings `HEAVY_UNITS` was derived from in a test-only record,
`MEASURED`): each test target (`--lib`, `--bin <name>`, `--test <name>`, `--doc`, selected over
`--workspace`, so the build and its features are the unsharded run's) and each played scenario falls
to the shard a salted hash of its name picks (`xtask/src/shard.rs`). Timing enters in one place
only: `HEAVY_UNITS`, the units that each take more than a tenth of a leg's test time on some leg in
some recorded run (a rule xtask's tests hold), and through it the salt, the smallest one that sets
those units on shards of their own, the heaviest not on shard 1 (decisions-log item 194). xtask's tests check that the split
is a partition, every unit on exactly one shard, over this workspace's own metadata, and that the
units mirror `cargo test`'s default selection, and that the leg job counts a shard with no report,
or one that did not run and pass its suite, as failed; the pull request that introduced it compared the test executables
cargo builds both ways. A target the split cannot select the same way (an example, a bench `cargo
test` would build, a target whose `test` or `doctest` flag is off) is a failure, never a target the
shards stop testing. The leg's own job, `cargo xtask ci (<os>)`, is the required check under the
name it always had, needing only its own operating system's shards: it fails unless every shard of
its leg reported running and passing its suite, merges their fresh outputs (a file two shards wrote
must be byte-identical), runs `cargo xtask golden` over them and uploads the `cross-os-<os>`
artefact the cross-OS guard compares. A leg's time, for the budget, is its slowest shard plus its
leg job (decisions-log item 148). `--shard` goes with `ci` alone and refuses `--quick`, `--package`,
`--fix` and `--bless`; without it, `cargo xtask ci` runs every step, as it does locally and in the
merge train's local check.

> **Status.** `xtask/src/main.rs` is green, and its sixteen steps cover items 1–10 below. Every step
> is required: a step whose input is missing fails rather than skips (decisions-log item
> 116 (6)(g)). A test that needs a tool (`crates/proto/tests/generated.rs` needs buf and
> `protoc-gen-prost`) still skips, and lets its step report `ok`, unless
> `PHARMAKOS_REQUIRE_TOOLS=1` is set, as it is in CI and in the merge train's local check. Item 11's budgets arrive with the gates that
> set them: S1's P1, S2's G3′-real, and S3's G2 + P3 and P1's end-to-end bar. Each gate is certified
> only by recorded runs on the owner's machine (S3's timed figures were certified on 2026-11-27: decisions-log item 223),
> and their CI alarms, `cargo xtask g3-budget`, `cargo xtask
> p3-budget` and `cargo xtask p1-budget`, sit outside the step table and can go red (item 11);
> every `perf-alarms` figure is published, never gated. The owner
> signs off the final step list, and the numeric budgets are Tuning values that the gates in spec
> section 16 set.

1. **Format** — `cargo fmt --all --check`.
2. **Lints** — `cargo clippy --workspace --all-targets -- -D warnings`, run once with default
   features and once with `--features research`. Never `--all-features` (it would enable `research`
   everywhere and hide the ban). Determinism lints: float arithmetic, `as` conversions, disallowed
   types (`HashMap`, `HashSet`, `Instant`, `SystemTime`), and the walled-module allowance audit.
   Clippy's calls carry the host's pinned linker, as every checked build does (item 5), so the
   units the two share are not rebuilt between them.
3. **Dependency policy** — the research-feature ban (§3 rule 1) by walking `cargo tree -e features`;
   the no-dry-runs ban (`plan-core` and `verifier` never enable `research` and never reach the sim's
   stepping API); the banned-crate list; `cargo deny` for licences and advisories.
4. **Profiles** — `[profile.release]` in `Cargo.toml` must set `overflow-checks = true`, and the
   step fails if any profile in `Cargo.toml` or `.cargo/config.toml` switches it off (a custom
   profile inherits `dev` or `release`, which cargo enforces); it does not require `[profile.dev]`
   to set it. A `[profile.*]` line the scan cannot read fails, and every
   `[profile.dev.package.<name>]` table must name a package cargo resolves, with only the godot
   family at opt-level 0. The checked builds (item 5) then read back from cargo that every
   workspace unit has overflow checks and debug assertions on (decisions-log item 165).
5. **Tests** — `cargo test --workspace` without `research`, and `pharmakos-sim`'s own tests with it
   (`cargo test --package pharmakos-sim --features pharmakos-sim/research`), each run through the
   checked call: xtask hands cargo the command and reads the profile cargo resolved for every unit
   back from cargo's own report, which the test, `determinism` and `package` builds all take. Every
   test executable and every doctest is started through xtask's own runner (`xtask/src/runner.rs`),
   which records each start; a step fails when a test target the build reported did not run, or
   ran from a file the build did not report, and a doctest run is one process per doctest. The
   linker is pinned per host triple and the rustc wrappers are off, on xtask's command line and in
   `.cargo/config.toml`, so inside this repository a user-level linker or wrapper setting does not
   apply; `cargo xtask` runs its builds, lints and tests on the three CI host triples and refuses
   another host when it builds, lints or runs tests, naming the row to add to its linker table and
   to `.cargo/config.toml` (a plain `cargo` works on any host). Every command except the hidden
   runner `__run-test`, which cargo starts and which is dispatched first, checks at the door that
   the pinned `rustc` and `$CARGO` report `rust-toolchain.toml`'s release, so `cargo +other xtask`
   is refused. A plan that runs tests is refused when xtask's own path (`<target>/xtask/xtask`)
   holds whitespace or a quote in both its checkout-relative and its absolute spelling, which is the
   case for a target folder outside the checkout whose path holds one. `cargo xtask checked-test
   --package <name> --test <target> [--ignored] [--nocapture]` runs one test target through the same
   call (the nightly fuzz job's), and `cargo xtask determinism-run` and `cargo xtask build-gamectl
   [--release]` are the nightly and watch jobs' checked builds; none is a step. The suite builds the
   sim and the third-party crates it runs at opt-level 2, checks on, except the allocation test,
   which runs as an invocation of its own with the sim at opt-level 0 (decisions-log item 165). The
   sim is the one crate
   whose code the feature changes, and clippy's research pass (item 2) still compiles every
   non-walled crate against a research sim (decisions-log items 113 (9) and 115 (5)). The
   allocation test's tick tests (`crates/sim/tests/allocations.rs`, module `tick`) are built only
   without `research`, since the feature gates `pub mod research;` alone and enables nothing; that
   file's `the_research_feature_gates_the_fork_module_and_nothing_else` runs in both builds and
   fails if that stops being true. A fork's tick and a restored world's tick are
   allocation-tested too: that file's `tick::a_restored_worlds_tick_allocates_nothing` runs a world
   restored from a save through a growth window and a fight at zero allocations, its chain equal to
   the unbroken run's, the two runs stepped side by side on two scoped threads (the counter is
   thread-local, which `the_counter_counts_each_thread_alone` tests), and its `fork` module, in the research build, holds a forked world's tick, its
   parent's while the fork lives and a fork of a restored world's to exactly the chunks each
   re-points, tick by tick, counted by the chunks' addresses (decisions-log items 189, 201 and 209). The
   file's `the_counting_allocator_counts_every_hook_it_says_it_counts` tests its own counter, in both
   builds. An unscoped
   run first empties `<target>/golden/`, so every `actual.*` the `golden` step reads was written by
   this run; a `--package` run empties nothing (decisions-log item 131). A check run by hand, such
   as `crates/gamectl/tests/balance.rs`'s 80-match balance report, is an `#[ignore]`d test: never a
   step, and never compared across operating systems (decisions-log item 130). The tests include
   xtask's `the_clone_runs_the_versioned_hooks`, which fails a clone whose `core.hooksPath` is not
   `scripts/githooks`, except where `CI` or `GITHUB_ACTIONS` is set (decisions-log item 132).
6. **Determinism** — replay a fixed set of seeded matches headless and compare the full per-tick
   xxh3 hash chain against golden files; save/restore round-trips hash-identically; in the research
   build, fork equivalence. The CI matrix runs Windows, Linux and macOS and compares the chains
   across all three (gate G4): the `cross-OS determinism guard` job compares, byte for byte, each
   leg's determinism chain, every scenario chain (and `expand-east-segment`'s event log), the
   path-hash file, the mesher, mapgen and vista geometry digests and every verifier report (each
   case's report and the catalogue, the at-budget `budget_128` report named so that a leg that wrote
   none is red), requires each on all three legs, and compares no perf figure (decisions-log items
   116 (6)(n) and 218).
7. **Goldens** — JSONC playbooks round-trip byte-for-byte through `plan-core`'s canonical form
   with comments intact; verifier `report_hash` goldens; the diagnostic catalogue; `render_plan`
   prose. Golden diffs must be human-readable. `cargo xtask golden --bless` rewrites the
   `expected.*` files that exist and creates none: a new case's golden is its fresh `actual.*`
   copied into place (a new scenario's, `gamectl scenario run`'s `actual.hashes.txt`). A producer
   that stops at its first mismatch (`crates/gamectl/tests/scenarios.rs`, and
   `crates/gateway/tests/demo_playbooks.rs`, which writes its render output after its verify
   comparison) leaves later outputs unwritten, so after a bless it is re-run and blessed again until
   the `golden` step reports nothing missing (decisions-log item 133).
8. **Schema** — `buf lint`, and `buf breaking` against `main` in `WIRE_JSON` mode (both run from the
   workspace root with `proto` as the input, because buf resolves a `.git#…` reference relative to
   the invocation directory); a second comparison against the last release tag is added when v1.1
   publishes `gp.api.v1`. The generated prost tree, `get_schema`'s answer and `gamectl docs`
   output are regenerated and must match what is committed (so they cannot drift); that half runs
   as tests through the `test` and `golden` steps (`crates/proto/tests/generated.rs`,
   `crates/gateway/tests/methods.rs`, `crates/gamectl/tests/docs.rs`), not in the `buf` step, and
   generated JSON Schema joins it with v1.1. The `buf` step fails, rather than skipping `breaking`,
   when there is no local `main` to compare against (decisions-log item 116 (6)(g) and (p)).
9. **Licensing** — REUSE check: every file has an SPDX header or a `REUSE.toml` entry, and the
   manifest is complete, and every tracked file outside `library/` that reproduces a template's
   expression carries the library's licence (§8; `docs/LICENSING.md` states the rule and
   `xtask/src/licence_quotes.rs` runs it);
   the `LICENSE` pointers are §8's rule, which no step checks. The
   `placeholders` step, `cargo xtask placeholders --check`, runs after it and fails on any
   `PLACEHOLDER` marker off the register's grammar (below; decisions-log item 137).
10. **Scenarios** — the `scenario` step validates every committed scenario file and plays each with
    `gamectl scenario run` (map seed + playbooks + assertions on events and hashes). The
    `screenshot` step renders nine shots (`SHOTS` in `xtask/src/main.rs`), the vista, the diagnostic
    rows, the wizard's first page, the ring, the lobby, the template list, Playtest mode, the rule list
    and the condition picker, the last six drawn from committed fixtures with no host (decisions-log
    item 218), windowed under xvfb with lavapipe, on Linux only (in CI, the `vista screenshot (linux)` job), and
    compares each with its PNG golden (skeleton-plan decision 22; decisions-log item 116 (6)(a) and
    (h)); `--headless` selects Godot's dummy renderer, which cannot take a screenshot, so the step uses
    it only for the import. The headless watch check (`godot/scripts/watch_check.gd`, run against a real
    `gamectl host`) runs only in CI's `client extension` jobs, outside `cargo xtask ci`, so a change to
    `godot/scripts/` can be green locally and red there; the lane that makes one runs it by hand with the
    job's command (decisions-log item 143 (2)). The adversarial scenarios are nightly (below) and gated off until S2.
11. **Perf budgets** *(added per gate, not in `cargo xtask ci`)* — G3′-real (spec §16's G3′ row:
    a tick p99 of at most 25 ms and a mean of at most 12.5 ms) is certified by recorded runs on the
    owner's machine under Windows and WSL2 Linux, the slower deciding (S2's plan, section 5; `g3r`
    #147, re-certified on `tune2`'s rules by `demo2`, decisions-log item 151, and with raids on
    `tune3`'s by the certifying runs of decisions-log item 223). Its CI alarm is
    `cargo xtask g3-budget` (`xtask/src/g3.rs`, E18), which is not a step: it plays the segment once
    on a hosted runner and fails only at the gate's numbers doubled (a p99 over 50 ms or a mean over
    25 ms) or on any missing verdict, in the `g3 budget (ubuntu-24.04)` job, which is not required
    but **can go red**, and `scripts/merge-train.sh` stops on it. G2 + P3 (a repath p99 of at most
    5 ms and a one-leg estimate p99 of at most 1 ms) and P1 (QUICK at most 5 ms and FULL at most
    50 ms p99 at the playbook size budget, and the editor's end-to-end wait at most 50 ms p99,
    decisions-log item 162 (1)) are certified only the same way, by recorded runs on the owner's machine
    under both systems (certified on 2026-11-27: decisions-log item 223), and their CI alarms are `cargo xtask
    p3-budget` and `cargo xtask p1-budget` (`xtask/src/budget.rs`, E12), not steps either, in the
    `authoring budgets (ubuntu-24.04)` job, which fails only at those numbers doubled or on any
    missing verdict (a bar or a plan that is not xtask's own is one), is not required, **can go
    red**, and stops `scripts/merge-train.sh` as `g3 budget` does. Every certifying command runs
    with `PHARMAKOS_BENCH_CERTIFY=1`, read strictly (exactly `1`, or unset; any other value refuses
    the run), which no workflow sets (xtask's `no_workflow_asks_a_bench_run_to_certify`); without
    it, or when `PHARMAKOS_BENCH_CERT_CALLS` cuts a count, every line the command prints carries the
    "NOT A CERTIFYING FIGURE" banner (decisions-log item 218). Beside them,
    `cargo xtask perf-alarms [mesher|p1|p1-cap|p1-decision|p1-kinds|p3|g3|g3-watch|allocations]`,
    which is not a step, publishes the mesher p99, P1's figures from `pharmakos-bench` (QUICK's p50
    and p99 over the committed cases and at `budget_128`, the figures at the transport cap, ns per
    size unit per decision tick, and the decision tick's ns per counted unit by kind), P3's,
    G3′-real's figures and its watch (the view feed, each seat's peak sightings and the breach
    search at the gate's load), and the allocations
    per tick (the count `crates/sim/tests/allocations.rs`'s `tick::a_tick_allocates_nothing`
    asserts, which the `test` step fails without libtest reporting it passed, once) as `::notice::`
    annotations per runner, with no threshold and never compared across operating systems, in the `perf alarms (…)`
    jobs: one step per alarm, because GitHub annotates only a step's first ten notices (decisions-log
    item 149 (4)), each continuing on error, so a failed measurement is a `::warning::` and the job,
    which is not required, never goes red (decisions-log items 116 (6)(b) and 131). With no name it
    runs all nine. `pharmakos-bench` is the walled harness that drives the sim from outside
    (§4.5).

**The docs-only fast path.** A pull request whose merge commit changes only `docs/**`, `AGENTS.md`,
`CLAUDE.md`, `.claude/**` and top-level `*.md` runs the DCO walk and the `reuse` step in place of the
suite; either can still fail it. Every other required job skips its remaining steps and reports
success; each job that builds, and the cross-OS guard, says so in a `::notice::`. Each building
job's scope step decides (each shard, not the leg's `cargo xtask ci (<os>)` job, which takes prose
only when every one of its shards' reports says so): it answers `full` in bash,
before it builds `xtask`, for a merge commit that changes `xtask/`, `.github/` or `.cargo/`, so the
decider never judges its own change; otherwise `cargo xtask ci-scope` decides, and answers `full` on
any event but `pull_request` and on any error. Nothing can force it. The `perf alarms (…)` jobs, the
`g3 budget (ubuntu-24.04)` job and the `authoring budgets (ubuntu-24.04)` job, which are not
required, follow the same pattern. The full suite runs on `main`'s push
(decisions-log items 115 (4) and 116).

**The package.** `cargo xtask package` is not a step either: `cargo xtask ci` never runs it, and it
runs alone (decisions-log item 117). It builds this platform's unsigned zip: the client library
with `[profile.release-client]` and `gamectl` with `--release`, both for the host triple (on Windows
with `+crt-static`, and a byte scan of both for the C runtime's names); a copy of `godot/` with the
library staged into it, imported; the shipped smoke check, run once in that copy under the editor;
the export; a byte scan of the `.pck` for test files; the zip, whose entry list it compares with
`tests/golden/package/` (`--bless` rewrites the list); `reuse lint` over the extracted zip; and every
committed scenario, replayed by the packaged `gamectl` against its golden chain. It fails rather
than skips when Godot, the export templates, `reuse` or (on Linux) `objdump` is missing, and it
refuses on macOS, which has no package. CI's `package (windows-latest)` and `package (ubuntu-24.04)`
jobs run it and upload the zip; `clean launch (windows-latest)` and `clean launch (ubuntu-24.04)`
extract that zip on a runner with no checkout and no toolchain, and run `gamectl seat doctor`
inside it and the smoke check from the parent folder. The four have been required checks since T21
merged (items 119 (1) and 121), and they take the fast path: the package jobs decide as the
building jobs do, and each clean-launch job reads its package job's answer. A lane that changes
`library/`, `rules/`, `packaging/`, `LICENSES/`, `REUSE.toml` or `godot/` changes what ships, so the
main session runs `cargo xtask package` on it locally before it merges (S1's plan, decision 9,
ruled by decisions-log item 128). The command keeps the smoke run's user folders under
`<target>/package/data/` itself, and reads the export templates from Godot's own folder, where they
must be installed.

**The PLACEHOLDER register.** `cargo xtask placeholders` collects every
`PLACEHOLDER` marker in the tracked tree, parses each against the one-line grammar
`<what> — <who>, <when>`, prints the register grouped by when and by who, and with `--check` fails
while any marker is off the grammar; without `--check` it lists the register and never fails.
Since S1's `tune`, `cargo xtask placeholders --check` is one of the sixteen steps, `placeholders`,
run after `reuse`, and it fails on any marker off the grammar (decisions-log items 128, 129 and
137).

Nightly, additionally: the three adversarial scenarios (§10) on a fixed seed set, and the fuzzer
(10 000 generated playbooks, no panic). Both are jobs in `.github/workflows/nightly-scenarios.yml`
behind its `NIGHTLY_SCENARIOS_ENABLED` variable, which stays unset until S2 (decisions-log item
116 (6)(j)); a manual run's `force` input runs them regardless.

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
  out-of-vocabulary construct with a code and a JSON Pointer and never silently strips it. A report
  lists at most `MAX_PER_CODE` diagnostics of one code, so past that many the rest are counted in
  the report's typed `left_out` field (`VerifyReport.left_out`, one `LeftOut` a code with its
  `code`, `count`, `max` and `severity`) and said by its closing `I0007`, rather than each given its
  own pointer; a report lists the first `max` of a code in the order the verifier raises them
  (`crates/verifier/tests/report_size.rs`'s `a_report_lists_the_first_max_of_a_code_in_raise_order`);
  the gateway sends the field as the verifier wrote it (`crates/gateway/tests/left_out.rs`), and the
  built-in operator's tap-probe budget is held to the cap at compile time
  (`crates/gamectl/tests/report_cap.rs`'s two checks of `MAX_PER_CODE` against
  `TAP_PROBE_LISTED_FLOOR` and `TAP_PROBE_VERIFIES`).
  A member name of the player's own too long for a pointer (`MAX_POINTER_BYTES`) is pointed at by
  the object that holds it (decisions-log items 183 (4), "Cap all codes", 203 and 214).
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
- When you must guess, write `// PLACEHOLDER: <what> — <who>, <when>` (an em dash with a space on each side, the
  grammar `cargo xtask placeholders --check` parses) and list it in your PR.
