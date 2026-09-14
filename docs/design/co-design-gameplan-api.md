# Gameplan, Verifier, Planning API and Editor: co-design v1

Status: design draft for owner review. Date 2026-09-12.
Source of truth: `decisions-log.md` (section 2.0 overrides everything earlier). Where this document has to interpret the log, it says so and the matter is listed in §7 (open questions) or §8 (contradictions).
Note on paths: the log was found at `...\3fbcdbe6-...-6cefb998-...\scratch-2026-09-12-daaf8d\decisions-log.md` (hyphen-joined folder), not the backslash path in the brief. `switchyard-spec.html` next to this file is the superseded Mindustry straw man and was ignored, as the log instructs.

---

## 0. Design stance in one page

**What a gameplan is.** A gameplan is a *typed, declarative commander program* for one 8-minute play segment. It contains:
- a **route**: an ordered list of steps (move, interface with a beacon, place a beacon, wait, broadcast), and
- a **gambit list**: prioritised handlers that can pre-empt the route when a seat-knowable condition becomes true.

The commander is the only thing a plan drives directly. Everything else changes through beacon interfaces (on site, timed, interruptible) or through data-only radio broadcasts.

**Why a plan can survive 8 minutes without dry runs.** There is no single clever feature; five mechanisms work together:
1. **Late-bound targets.** Selectors such as "my weakest Defend beacon" or "nearest own beacon that is under attack" resolve at execution time, not when the plan is authored. A plan written against a stale map still does something sensible.
2. **Interrupt-and-resume handlers** with explicit resume semantics (`CONTINUE`, `RESTART_STEP`, `SKIP_STEP`, `GOTO` a forward label, `END_ROUTE`). A raid causes a detour, not a broken plan.
3. **Guards on every step.** `skip_if` and `require` guards, plus `on_fail` policies, let a step that no longer makes sense (beacon destroyed, target captured) degrade instead of blocking.
4. **Beacon-side autonomy set during the interface.** Mandate settings carry their own rules: rules of engagement, retreat HP, a launch condition such as "at segment time 4:00 or at 12 raiders". Most of the adaptation therefore happens inside operators, which run every tick. The commander only has to deliver the right settings.
5. **A guaranteed tail.** When the route ends or fails, the commander follows a `fallback` posture (hold at a safe anchor, shadow a beacon, or patrol), and a built-in self-preservation reflex sits above every handler.

**One engine, three clients.** The Rust core provides `verify()`, `estimate()` and `render()` as pure functions. The Seat Gateway exposes them. The Godot editor, the MCP adapter and the built-in planner all call the same gateway methods. The editor never does its own validation or time maths: it shows what the core returns.

**Deterministic by construction.** The knowledge snapshot is frozen when the pause starts. Verification is a pure function of `(plan bytes, snapshot id, rules hash, verifier version)`. A pre-verify by an LLM and the verify at submit therefore give byte-identical reports.

---

## 1. GAMEPLAN SCHEMA v1

### 1.1 Execution model (normative)

**Clock and cadence**
- Plan time is measured in game milliseconds from segment start, `seg_ms` in [0, 480000].
- The sim converts milliseconds to ticks using the rules table. The schema never mentions ticks, so a future change of tick rate does not break plans.
- **Tick rate assumption: 20 Hz** (1 tick = 50 ms). The log never states a rate; the 60 Hz figure comes only from the superseded straw man. This is flagged as OQ-1. All schema values are multiples of 50 ms, and the verifier rounds up with I0002 when they are not.
- The plan interpreter runs inside the deterministic sim, once per **decision tick**, which is every 250 ms of game time, or 5 ticks (the rules constant `PLAN_EVAL_PERIOD_MS`).
- Movement and interfacing progress every sim tick. Decisions (handler checks, step transitions, selector resolution) happen only on decision ticks. This bounds CPU and makes timings reproducible.

**State**
The interpreter tracks:
- a program counter into `route`
- a handler stack, at most 1 deep (a handler cannot pre-empt another handler, except the reserved self-preservation reflex)
- flags: at most 16 named booleans
- counters: repeat iterations, and fire counts per handler
- `posture`, one of `ROUTE`, `HANDLER`, `FALLBACK`, `DOWN` (dead and waiting to respawn), `DONE`

**Decision tick order** (fixed)
1. **Reflex.** If a self-preservation condition holds (commander HP at or below `limits.reflex_hp_pct`, default 20), abort any interface and path to the reflex anchor. The reflex cannot be disabled; its threshold can be lowered to 10.
2. **Handlers.** Scan in list order and pick the first handler that is enabled, off cooldown, has fires left, and whose condition is true.
   - If a handler is already running, only handlers flagged `preempt_handlers: true` that sit *above* it may take over. This is the gambit rule: "first true wins, top is highest priority" ([FF12 gambits](https://finalfantasy.fandom.com/wiki/Gambits)).
   - A pre-empted handler is dropped, not resumed. Its resume policy is not applied; the new handler's policy applies.
3. **Current step.** If no handler took control, advance the current route step (or the running handler's body): evaluate completion, and transition when done.

**Handler entry and exit**
- When a handler fires, the interpreter saves the `ROUTE` program counter and any in-progress interface is **aborted**. Changes that have already committed stay; the rest are lost, per the interruptible-interface rule.
- When the handler body finishes, the handler's `resume` policy applies:

| resume | effect |
|---|---|
| `CONTINUE` | Return to the saved step, re-entering it from its start. A `move` step re-paths; an `interface` step redoes only the changes that have not committed. |
| `SKIP_STEP` | Go to the step after the saved one. |
| `GOTO` | Jump to `resume_label`. It must be at or after the saved step (verifier check E0310). |
| `END_ROUTE` | Enter `FALLBACK`. |

**Death (a required fallback)**
- Every plan **must** contain an `on_death` block. A plan without one fails with E0110. Editors and templates always insert the default, so humans never see this error; it exists so that LLM-authored plans make the choice explicitly.
- `on_death` fields:
  - `on_respawn`: one of `CONTINUE`, `SKIP_STEP`, `GOTO` a label, `END_ROUTE`
  - `respawn_steps[]`: at most 4 steps, run first after respawn. Typical use: "walk to the safest beacon, then hold 5 s".
  - `max_deaths_before_fallback`: 1–3. After this many deaths, go straight to `FALLBACK`.
- On death: `posture = DOWN`, handlers are not evaluated, and any interface is lost. The respawn delay grows with repeated deaths, per the log.
- Beacons keep running their configurations while the commander is down. The verifier's timeline shows the respawn delay as a hatched gap, for the worst case only when the author asks for it.

**Route end**
- When the route ends, or a step fails with `on_fail: ABORT_ROUTE`, the plan enters `FALLBACK`.
- Handlers still run during fallback. When a handler finishes in fallback, it returns to fallback.

**Segment end**
- The interpreter stops. Nothing carries into the next segment except the world itself: beacon configurations and positions.

### 1.2 Step types

| Step | Completes when | Fails when (then `on_fail`) |
|---|---|---|
| `move` to a location | the commander is within `arrive_radius` | no path; timeout reached |
| `interface` with a beacon: a list of changes | all changes committed | beacon lost, contested, or out of range; timeout |
| `place_beacon` | deploy finished | placement becomes illegal; timeout |
| `wait_until` a condition | the condition is true | `timeout_ms` reached. **Required**, and it follows the timeout policy |
| `hold` for a duration | the duration has passed | never |
| `broadcast` a data message: `MARK_TARGET`, `THREAT_AT`, `GO {code}`, `RALLY_AT` | sent | messaging capability not `ACTIVE`; commander outside mast coverage. Then `on_fail`. |
| `set_flag` / `clear_flag` | immediately | never |
| `branch`: if a condition holds, go to a forward label | immediately | never |
| `repeat` a body N times (N ≤ 8, body ≤ 8 steps, no nesting beyond depth 2) | N iterations, or `until` true | never; each iteration must contain a step that consumes time (E0321) |
| `checkpoint` label | immediately | never; a no-op anchor for `GOTO` and `branch` |

Guards available on every step:
- `skip_if` (a condition): if it is true on entry, skip the step.
- `timeout_ms`: optional on most steps, required on `wait_until`, and implied by the segment everywhere.
- `on_fail`: `SKIP` (default), `ABORT_ROUTE`, or `GOTO` a label.
- `label` and `note` (free text, at most 120 characters, shown in the editor).

**Messaging decision (resolves "no sender in v1").** Before the Messaging capability is earned and its Radio Mast is built, there is **no sender at all**:
- the commander cannot broadcast
- operators only *receive*, and in practice receive nothing
- Attack's `broadcast_received` launch option and every `broadcast` step are inert. The verifier reports W0611, and the step is skipped.

Once Messaging is `ACTIVE`:
- (a) the commander can `broadcast` while inside a mast's coverage; and
- (b) default operators start sending their own data messages (threat and help requests), exactly as the log says.

So the gameplan *can* send messages, but only through the earned capability. This makes Messaging worth fighting for: it unlocks go-code synchronisation and target marking without on-site visits.

Messages never reconfigure beacons. They only feed inputs that were already configured on site: `retarget: MARKED_ONLY`, `launch.broadcast_received`, and Defend's threat weighting.

`on_fail: RETRY` is intentionally absent. A retry is a `repeat` with `until`, which keeps loops visible and bounded.

### 1.3 Targets: references and selectors

Everything a step points at is a `BeaconRef`, a `LocationRef` or an `AreaRef`. Each is a oneof of a **fixed reference** or a **selector**.

**Fixed references:** `beacon_id`, `voxel` (x, y, z integers), `waypoint` (a named point in the plan), `beacon_anchor` (a beacon's interface point).

**Selectors** resolve at step entry and are then pinned for that step, which prevents flip-flopping. Tie-break: lowest ETA, then lowest id.

| Selector | Picks |
|---|---|
| `own_beacon {filter, order}` | Filter by mandate type, status (`ACTIVE`, `DORMANT`, `UNDER_ATTACK`, `CONTESTED`), tag (a plan-local alias set in `meta.beacon_tags`), minimum funding forecast, or has capability structure. Order by `NEAREST`, `WEAKEST_HP`, `MOST_THREATENED`, `LOWEST_FUNDING`, `OLDEST_CONFIG`. |
| `known_enemy_beacon {filter, order}` | Only beacons in seat knowledge (visible now or last known). Filter by `last_seen_within_ms` and `max_distance_from`. |
| `safest_own_beacon` | Own beacon with the highest defence score and the lowest threat, reachable. |
| `spawn_point` | Respawn location. |
| `offset {base, dx, dy, dz}` / `toward {from, to, distance}` | Geometry helpers, for staging just outside a sphere. |

Every selector has `none_policy`: `FAIL_STEP` (default) or `SKIP_STEP`.

### 1.4 Condition vocabulary (seat-knowable only)

**Structure**
- `Condition` = `all[]` | `any[]` | `not` | a leaf predicate.
- Maximum depth 4 and at most 24 nodes per condition (E0202).
- Leaves compare integers with `LT`, `LE`, `EQ`, `GE`, `GT`, `NE`. Percentages are integers 0–100; money is whole $; durations are ms.
- No floats and no arithmetic. There is one relative form, `delta_since_step_entry` (for example "treasury dropped by ≥ 300 since this step began").

**Knowledge rule**
- A leaf reads only from the **seat knowledge store**: fog-filtered current vision from own beacons and units, last-known memory with timestamps, own reports, own economy, own capability state, and public match state (clock, round, standings as shown in the recap).
- Predicates about enemies take a `freshness` argument: `VISIBLE_NOW` or `KNOWN_WITHIN {ms}`. A predicate cannot read hidden state; the store physically does not contain it.
- The verifier rejects enemy predicates without explicit freshness (E0205). This makes the fog semantics visible to authors.

**Leaf predicates**

| Family | Predicate | Arguments |
|---|---|---|
| Clock | `seg_time` | cmp, ms |
| | `round` | cmp, n |
| Commander | `cmdr_hp_pct` | cmp, pct |
| | `cmdr_in_sphere` | beacon ref |
| | `cmdr_near` | location, radius |
| | `cmdr_interfacing` | — |
| | `cmdr_deaths_this_segment` | cmp, n |
| | `cmdr_took_damage_within` | ms |
| Own beacon (by ref) | `beacon_status_is` | status set |
| | `beacon_hp_pct` | cmp, pct |
| | `beacon_under_attack_within` | ms |
| | `beacon_units` | role (`any`, `builder`, `combat`, `miner`, `repair`, `scout`), cmp, n |
| | `beacon_mandate_is` | mandate type |
| | `beacon_funding_forecast` | `COVERED`, `AT_RISK`, `UNCOVERED` |
| | `beacon_pool` | cmp, $ (mined $ this round) |
| | `beacon_structure_count` | kind, cmp, n |
| | `beacon_report_since` | report kind, window (`SEGMENT`, `STEP`, `{ms}`) |
| Aggregates | `own_beacons_count` | status filter, cmp, n |
| | `own_units_count` | role, cmp, n |
| Enemy knowledge | `enemy_units_near` | area, cmp, n, freshness, kind filter |
| | `enemy_beacon_known` | filter, freshness |
| | `enemy_structure_near` | area, kind, freshness |
| Economy | `treasury` | cmp, $ |
| | `upkeep_coverage` | `ALL_COVERED`, `SOME_AT_RISK` |
| | `income_forecast_this_round` | cmp, $ |
| Capability | `capability_state` | cap id, `NOT_EARNED`, `EARNED_UNBUILT`, `ACTIVE`, `SUSPENDED` |
| | `capability_rounds_left` | cap id, cmp, n |
| Radio (MastOnly v1) | `in_radio_coverage` | location or commander |
| | `beacon_in_radio_coverage` | beacon ref |
| Plan | `flag` | name |
| | `step_reached` | label |
| | `handler_fired` | handler id, cmp, n |
| | `posture_is` | posture |

Deliberately **absent**:
- anything about enemy plans, enemy treasury, or enemy mandates (these are not knowable)
- exact unseen enemy positions
- "an enemy is targeting X" (intent is not knowable)
- random numbers

`beacon_report_since` is the workhorse. Operators already emit reports ("blocked", "under attack", "quota met", "budget exhausted" in the log's mandate draft), and the seat receives all reports anywhere. Conditioning on reports gives the plan the operators' local judgement for free.

### 1.5 Beacon changes (interface payload) and interface-time model

An `interface` step carries an ordered list of `BeaconChange`. Changes commit **one at a time, in order**. Damage interrupts the change in progress; earlier changes stay committed. This matches the decision that "the beacon keeps its old configuration" for the interrupted change, and gives authors a simple lever: put the most important change first.

**Timing**
- Duration is deterministic from the rules table `interface_costs` (versioned inside `rules_hash`).
- Each visit pays a handshake of 1500 ms once.
- Each change then costs:

| Change | Cost (ms) | Notes |
|---|---|---|
| `set_priority` (low, normal, high) | 1500 | Quartermaster knob |
| `edit_settings` (field-path patch) | 2000 + 500 × (fields − 1), max 6000 | about 3 s for a typical tweak, as in the log |
| `set_mandate` (switch type, with full settings) | 8000 | about 8 s, as in the log. Keeping the same type counts as `edit_settings`. |
| `add_build_target` / `remove_build_target` | 2500 each | Blueprint references into the Build mandate's target list |
| `build_capability_structure` | 5000 | Queues the structure as a top-order Build target and reserves $. Construction by drones takes further time, which is not interface time. |
| `recycle` | 10000 | 50% refund applied at commit. Bound units re-home to the nearest own beacon. |
| `reassign_units` | reserved (v1.x) | |
| `install_operator` | reserved (v2) | 10000–30000 by size, as in the log |

`place_beacon` is not an interface change. It is its own step: 12000 ms deploy, and the commander must be still.

Visit total = 1500 + Σ costs, capped by `limits.max_changes_per_visit` (6). Beyond the cap the verifier emits E0412 and suggests splitting into two visits.

**Rules while interfacing**
- **Range and stillness.** The commander must be within `INTERFACE_RANGE` voxels (rules constant, suggested 4) and not moving.
- **Interruption.**
  - Any damage to the commander interrupts the change in progress, which restarts from 0 when resumed.
  - The beacon becoming `CONTESTED` or `DESTROYED` fails the step.
  - A handler firing aborts the step. Resume semantics are in §1.1.

**Why these values.** The ratio keeps knob tweaks cheap and mandate switches expensive. A realistic 8-minute plan fits 6–10 visits plus travel, so the choice of *where to go* is the strategy. Values are tuning data, not schema.

### 1.6 Mandate settings (built-in operator) as schema

Mandates share a common envelope. Reconciling the log's §3 draft (`{type, sphere, params, budget, priority, rules_of_engagement, report}`, plus `override policy`) with later decisions:

| §3 field | v1 status | Reason |
|---|---|---|
| `budget` | **Removed** | The Quartermaster auto-budget replaces it: no envelopes. |
| `priority` | **Moved** to the beacon, not the mandate | It *is* the Quartermaster knob (low, normal, high). It survives a mandate switch, and `set_priority` edits it alone. There is exactly one priority concept. |
| `override policy` | **Removed** | There is no manual control and no other v1 controller. Field number reserved for custom operators. |
| `sphere` | Implicit | A mandate always belongs to its beacon. |
| `report` | Kept, as `reports` | |
| `ROE` | Kept, as `roe` | |

Every numeric field has a range enforced by the verifier (E0501) and a default, so a template sets only what matters.

**Beacon-level config** (not part of the mandate)
- `priority`: `LOW`, `NORMAL`, `HIGH`
- `mandate`
- `operator`: `BUILTIN` only in v1. These are native Rust operators; §2a's "default AI as a WASM operator" is superseded by §2.0.

**Common mandate fields**
- `roe`: `HOLD_FIRE`, `RETURN_FIRE`, `ENGAGE_ON_SIGHT`
- `retreat_hp_pct`: 0–90, default 30
- `reports`: the set of report kinds to emit. The default is all; muting only reduces recap noise, and the seat still learns critical events (destroyed, captured, dormant).
- `accept_broadcasts`: whether this operator adapts to received data messages (default true)

**Build**
- `targets[]`: `{blueprint_id, anchor voxel, rotation, order}`, at most 16
- `repair_threshold_pct`: default 60
- `rebuild_destroyed`: default true
- `terraform`: `NONE`, `FILL`, `DIG`, `BOTH`
- `protected_areas[]`: at most 4
- `auto_fortify_requests`: bool, default false. Honoured by Defend beacons that have `auto_fortify` set.

**Defend**
- `protect[]`: structure or area refs; empty means the whole sphere
- `engagement_radius_pct` of the sphere: 10–100, default 100
- `pursue`: `NEVER`, `SPHERE_EDGE`, `LIMITED {voxels ≤ 32}`
- `auto_fortify`: bool
- `unit_mix`: `{combat_min, repair_min}`. The fabricator requests to the Quartermaster follow this.

**Attack**
- `target`: `known_enemy_beacon` selector, area, or structure ref. Re-resolved by the operator when the target is lost, using `retarget: NONE | NEAREST_KNOWN | MARKED_ONLY`.
- `staging_point`: a location inside the sphere
- `launch`: `any[]` of:
  - `force_at_least {role, n}`
  - `seg_time_at_least {ms}`
  - `broadcast_received {kind: GO, code}`

  This is the go-code equivalent. See [Door Kickers 2 go codes](https://steamcommunity.com/app/1239080/discussions/0/594015574338601221/): synchronisation by a shared clock or a signal. Here the signal is a radio broadcast, which is data only and so consistent with the "messages never reconfigure" rule, because the launch condition was already configured on site.
- `force_min {raider, charge_carrier, repair}`
- `retreat_when`: `any[]` of `losses_pct_at_least`, `target_destroyed`, `elapsed_ms_since_launch`
- `after`: `REGROUP_AT_STAGING`, `HOLD_TARGET_AREA`, `RETURN_HOME`
- `avoid_areas[]`: at most 4
- `breach_allowed`: bool
- `max_range_from_sphere`: voxels, capped by the rules. The log leaves Attack scope open; see OQ-6.
- `waves`: `SINGLE` or `REPEAT {max 4}`

**Mine**
- `resource_priority[]`: ore kinds (all ore converts to $ on delivery)
- `stop_at_pool`: $ (idle when the beacon pool reaches it), or 0 for no cap
- `dig_max_depth`
- `keep_pillar_every`
- `no_dig_under_structures`: always true, and not editable
- `flee_on_threat`: bool, default true
- Delivery: no field in v1. Mining drones always deliver physically to **their home beacon's storage**, which becomes that beacon's spendable pool for the round and then sweeps to the treasury, per the log. Field number 20 is reserved for `deliver_to` when logistics links arrive.

All four mandates reserve field numbers 100–199 for operator-specific extensions (custom operators in later versions).
### 1.7 Handlers (gambits)

A handler is `{id, label, enabled, when: Condition, body: Step[≤ 8], resume, cooldown_ms ≥ 5000, max_fires 1..8, preempt_handlers, only_in_postures}`.

**Body restrictions**
- A body cannot contain `repeat` nested deeper than 1.
- A body cannot contain `branch` or `GOTO` outside itself.
- A body cannot contain another handler.

**Limits**
- At most 12 handlers. This is the FF12 gambit count, a known-learnable size.
- The editor shows the first 6 open by default.

**Standard handler library** (templates insert these; they are plain handlers with no special engine code)
- **Flee when hurt.** `cmdr_hp_pct ≤ 40` AND `cmdr_took_damage_within 3000`. Move to `safest_own_beacon`, hold 8 s, then `CONTINUE`.
- **Rescue a beacon under attack.** `beacon_under_attack_within 10000` on any beacon tagged `core`. Visit it and interface: `edit_settings roe=ENGAGE_ON_SIGHT`, `set_priority HIGH`. Then `CONTINUE`. Cooldown 60 s, max 2 fires.
- **Fix funding.** `upkeep_coverage = SOME_AT_RISK`. Visit `own_beacon{order: LOWEST_FUNDING}` and interface `set_priority HIGH`. Max 1 fire.
- **Enemy near the commander.** `enemy_units_near {cmdr, r=24} ≥ 3 VISIBLE_NOW`. Move `toward(enemy → safest_own_beacon, 30)`, then `SKIP_STEP`.
- **Opportunistic capture scout.** Uses `enemy_beacon_known` with `status=DORMANT` (if dormancy is visible) or low HP. Needs the Attack beacon to have `retarget: NEAREST_KNOWN`. Only a broadcast can steer it, if Messaging is active.

### 1.8 Termination and bounded evaluation (guaranteed)

**Structural guarantees**, all checked by the verifier. A plan is rejected if any fails.
1. **Forward-only control flow.** `branch`, `GOTO` resume, `on_fail GOTO` and `on_respawn GOTO` targets must have an index at or after the source (E0310). The route is a DAG in program-counter order.
2. **Bounded loops.** `repeat.count` ≤ 8, and nesting depth ≤ 2. Every iteration path must contain at least one time-consuming step (`move`, `hold` ≥ 1000 ms, `wait_until`, `interface`, `place_beacon`) (E0321).
3. **Every wait has a timeout.** `wait_until.timeout_ms` is required and ≤ 480000.
4. **Expanded size.** Route steps × repeat multiplicity ≤ 256; literal steps ≤ 64; handler count ≤ 12; handler body ≤ 8 steps. This follows the "64 steps / depth 6" research limits: the steps limit is kept, and depth is tightened to 2 for loops and 4 for conditions.
5. **Handler fire budget.** `max_fires` ≤ 8 and `cooldown_ms` ≥ 5000, so total handler activations are bounded by Σ max_fires ≤ 96.

**Runtime guarantee**
- Every decision tick does bounded work: at most (1 reflex + 12 handlers × 24 nodes + 1 step) predicate evaluations. Each predicate is O(1) or O(log n) against knowledge indices maintained by the sim.
- Selectors run only at step entry, bounded by own beacons (≤ 32) or known enemy beacons (≤ 64).
- The hard stop is the segment end. Even if all of this were ignored, the interpreter halts at `seg_ms = 480000`.

**Result.** Every plan halts, and there is no zero-time infinite loop. Worst-case CPU cost per seat is a constant, which the verifier reports as `eval_cost_units` in the report.

### 1.9 Versioning and forward compatibility

**Identity**
- Package `gp.v1`. `Gameplan.schema_version` is `{major: 1, minor: n}`.
- A *minor* version adds optional fields or enum values.
- A *major* version means a new package (`gp.v2`), plus a server-side converter v1→v2 that stays forever.

**Strictness at the boundary**
- Unknown fields or unknown enum values in a *submitted* plan give **E0003** ("field not supported by this game's schema 1.3; you sent 1.4").
- The storage layer preserves unknown fields when reading plan files; the gateway does not accept them. This follows the protobuf guidance that unknown-field passthrough from untrusted inputs enables smuggling ([protobuf unknown fields](https://kmcd.dev/posts/protobuf-unknown-fields/), [proto3 guide](https://protobuf.dev/programming-guides/proto3/)).
- Every enum has `*_UNSPECIFIED = 0`, which is rejected by the verifier.

**Compatibility gate**
- The repo runs `buf breaking` with category **WIRE_JSON**, because plans travel as JSON and JSON breaks on renames ([buf breaking rules](https://buf.build/docs/breaking/rules/)).

**Extension seams** (numbers reserved now, so the later additions are additive)
- **WASM commander.** `Gameplan.body` is a oneof: `declarative = 10` (v1) or `wasm_commander = 11` (reserved). The outer envelope (header, `on_death`, fallback, limits, labels) is shared, so the verifier's envelope checks, the timeline and the safe-plan path keep working. This matches the log's "WASM commander later under the same outer format".
- **Custom operators.** `BeaconConfig.operator` is a oneof: `builtin = 1` or `program_ref = 2` (reserved; content hash plus ABI). The `install_operator` change is reserved at field 30. Mandate messages reserve 100–199.
- **Connectivity.**
  - `RadioModel` enum: `MAST_ONLY = 1` in v1; `HORIZON`, `RELAY`, `JAMMABLE`, `SATELLITE` reserved.
  - Conditions `in_radio_coverage` and `beacon_in_radio_coverage` are defined in terms of a *coverage query*, not mast geometry, so line-of-sight, relays and jamming change the answer, not the vocabulary.
  - Reserved predicates: `link_up(a, b)`, `jammed_at`, `satellite_pass_within`.
  - Reserved broadcast field `route_hint`.
- **Capability-gated vocabulary.** Every step, change and predicate has a `requires` annotation in the proto options (for example `requires = "cap.messaging"`). The verifier and the schema-slicing tool (`get_schema {for_seat: true}`) use it, so an LLM only sees vocabulary its seat can use, while authoring in advance stays possible with warnings.

### 1.10 Protobuf sketch (abridged but normative in shape)

```proto
syntax = "proto3";
package gp.v1;
import "gp/v1/options.proto";   // custom options: (gp.range), (gp.doc), (gp.requires), (gp.unit)

message Gameplan {
  SchemaVersion schema_version = 1;
  PlanHeader header = 2;          // seat, round, snapshot_id it was authored against, author_kind
  PlanMeta meta = 3;              // title, labels[], beacon_tags[], waypoints[], template_ref
  oneof body {
    DeclarativePlan declarative = 10;
    // WasmCommanderRef wasm_commander = 11;  reserved for a later version
  }
  OnDeath on_death = 20;          // REQUIRED (E0110)
  Fallback fallback = 21;         // REQUIRED; templates default to HOLD at safest_own_beacon
  PlanOptions options = 22;       // allow_dormant_beacons, reflex_hp_pct
  EditorMeta editor_meta = 90;    // UI-only (node layout, collapsed, template provenance); ignored by sim, size-capped
  reserved 11, 30 to 49;
}

message DeclarativePlan {
  repeated Step route = 1;        // ≤ 64
  repeated Handler handlers = 2;  // ≤ 12, list order = priority
  repeated Flag flags = 3;        // ≤ 16 declared names
}

message Step {
  string label = 1;               // [a-z0-9_]{1,24}, unique
  string note = 2;
  Condition skip_if = 3;
  optional uint32 timeout_ms = 4 [(gp.unit) = "ms", (gp.range) = {max: 480000}];
  FailPolicy on_fail = 5;
  oneof kind {
    MoveStep move = 10;
    InterfaceStep interface = 11;
    PlaceBeaconStep place_beacon = 12;
    WaitUntilStep wait_until = 13;
    HoldStep hold = 14;
    BroadcastStep broadcast = 15 [(gp.requires) = "cap.messaging"];
    FlagStep set_flag = 16;
    FlagStep clear_flag = 17;
    BranchStep branch = 18;
    RepeatStep repeat = 19;
    CheckpointStep checkpoint = 20;
  }
}

message MoveStep { LocationRef to = 1; uint32 arrive_radius = 2; MovePace pace = 3; /* DIRECT | AVOID_KNOWN_THREATS */ }
message InterfaceStep { BeaconRef beacon = 1; repeated BeaconChange changes = 2; /* ≤ 6, commit in order */ }
message PlaceBeaconStep { LocationRef at = 1; BeaconConfig initial = 2; repeated string tags = 3; }
message WaitUntilStep { Condition until = 1; uint32 timeout_ms = 2; /* required */ TimeoutPolicy on_timeout = 3; }
message RepeatStep { uint32 count = 1; Condition until = 2; repeated Step body = 3; }
message BranchStep { Condition if = 1; string goto_label = 2; }

message BeaconChange {
  oneof change {
    MandateSpec set_mandate = 1;
    SettingsPatch edit_settings = 2;   // repeated {path: "defend.pursue", value: Value}
    Priority set_priority = 3;
    BlueprintPlacement add_build_target = 4;
    string remove_build_target = 5;
    CapabilityStructure build_capability_structure = 6;
    RecycleBeacon recycle = 7;          // confirm: true required (E0520 otherwise)
    // ReassignUnits reassign_units = 8;  reserved
    // OperatorInstall install_operator = 30; reserved (custom operators)
  }
}

message BeaconConfig { Priority priority = 1; MandateSpec mandate = 2; OperatorRef operator = 3; }
message OperatorRef { oneof op { BuiltinOperator builtin = 1; /* ProgramRef program = 2; reserved */ } }

message MandateSpec {
  Roe roe = 1; uint32 retreat_hp_pct = 2; repeated ReportKind reports = 3; bool accept_broadcasts = 4;
  oneof type { BuildSettings build = 10; DefendSettings defend = 11; AttackSettings attack = 12; MineSettings mine = 13; }
  reserved 5, 6;   // were budget and override_policy (removed)
}

message Handler {
  string id = 1; string label = 2; bool enabled = 3;
  Condition when = 4; repeated Step body = 5;
  ResumePolicy resume = 6; string resume_label = 7;
  uint32 cooldown_ms = 8; uint32 max_fires = 9; bool preempt_handlers = 10;
  repeated Posture only_in_postures = 11;
}

message OnDeath { ResumePolicy on_respawn = 1; string goto_label = 2; repeated Step respawn_steps = 3; uint32 max_deaths_before_fallback = 4; }
message Fallback { oneof kind { HoldAt hold = 1; ShadowBeacon shadow = 2; PatrolBetween patrol = 3; } }

message Condition {
  oneof node {
    ConditionList all = 1; ConditionList any = 2; Condition not = 3;
    SegTime seg_time = 10; CmdrHp cmdr_hp_pct = 11; CmdrInSphere cmdr_in_sphere = 12;
    BeaconStatusIs beacon_status_is = 20; BeaconUnderAttack beacon_under_attack_within = 21;
    BeaconReportSince beacon_report_since = 22; BeaconUnits beacon_units = 23; BeaconFunding beacon_funding_forecast = 24;
    EnemyUnitsNear enemy_units_near = 40; EnemyBeaconKnown enemy_beacon_known = 41;
    Treasury treasury = 50; UpkeepCoverage upkeep_coverage = 51;
    CapabilityStateIs capability_state = 60;
    InRadioCoverage in_radio_coverage = 70;
    FlagIs flag = 80; StepReached step_reached = 81; HandlerFired handler_fired = 82; PostureIs posture_is = 83;
    // 90–199 reserved: connectivity (link_up, jammed_at), custom-operator reports, v2 predicates
  }
}

message BeaconRef { oneof ref { string beacon_id = 1; string tag = 2; OwnBeaconSelector own = 3; SafestOwnBeacon safest = 4; } NonePolicy none_policy = 9; }
message LocationRef { oneof ref { Voxel voxel = 1; string waypoint = 2; BeaconRef beacon_anchor = 3; Offset offset = 4; Toward toward = 5; Spawn spawn = 6; } }
```

Schema rules chosen for determinism and for LLM-friendliness:
- **No `map<>`**, because iteration order must be stable.
- **No `int64`**, because proto JSON turns it into strings, which confuses LLMs. Milliseconds and $ fit in `uint32`.
- **No `google.protobuf.Any`, `Struct` or `Timestamp`.**
- **`Value` in settings patches** is a tiny oneof (`int`, `bool`, `enum_name`, `ref`).
- **Canonical encoding.** Canonical bytes = prost encoding in field order. `plan_hash = xxh3(canonical bytes)`.

### 1.11 JSON Schema generation: Protobuf-first vs schemars (decision)

The log decides "Protobuf canonical schema with generated JSON Schema/MCP tools" but leaves open *how* the JSON Schema is produced.

| Criterion | A. Proto → `bufbuild/protoschema-jsonschema` as-is | B. Rust types + `schemars` as source | **C. Proto canonical + in-house descriptor → JSON Schema generator (recommended)** |
|---|---|---|---|
| Single source of truth | yes | no. There are two sources, since proto still exists for the wire format. | yes |
| Oneof rendering | Loose (all fields optional; oneof exclusivity often lost) | Excellent (`#[serde(tag)]` enums → `oneOf`) | `oneOf` of single-key objects, generated from descriptors |
| Descriptions, ranges, examples for LLMs | Comments only | Doc comments, `#[schemars(range)]` | Custom options `(gp.doc)`, `(gp.range)`, `(gp.requires)`, `(gp.example)` → `description`, `minimum`/`maximum`, `examples` |
| int64-as-string quirks | present | none | avoided by the schema rules |
| Schema slicing per tool and per seat capability | no | manual | yes (walk descriptors, drop `requires` not met) |
| Solo-dev cost | lowest | medium | about 400 lines of Rust in `build.rs` using `prost-reflect` |

**Recommendation: C.**
- LLMs author best against schemas that are small, have explicit `oneOf` discriminators, string enums, numeric ranges and a one-line description per field. They also do best with per-tool schemas rather than one 3000-line bundle. See Anthropic, [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents).
- `schemars` gives the nicest output but splits the source of truth away from the wire format that the log already chose.
- The Buf plugin ([protoschema-plugins](https://github.com/bufbuild/protoschema-plugins)) is a good fallback for day 1, and a reference test oracle.
- Validation is **not** done by JSON Schema at runtime. JSON goes through a strict pbjson parse, then `verify()`. The JSON Schema is for authoring and MCP tool input descriptions only, so any gap between schema and parser shows up as a friendly E0001/E0003 diagnostic, never a silent acceptance.
- A CI test round-trips every example and template through JSON → proto → JSON and through the JSON Schema validator, so drift between A/C output and the parser fails the build.

---

## 2. VERIFIER

### 2.1 Definition of "qualifying" in v1 (replaces the §2a WASM definition)

§2a defined qualifying as "compiles to ABI, allowed imports, floats disabled, memory caps, sandboxed dry run under fuel, mandate-contract-valid intents, content hash". That definition is for WASM programs and is **superseded** for v1.

**A v1 gameplan qualifies when all of these hold:**
1. It decodes strictly as `gp.v1.Gameplan` at a supported minor version (no unknown fields or enums).
2. It passes every structural limit and termination rule in §1.8.
3. Every reference resolves against the seat's frozen knowledge snapshot, or is a selector whose `none_policy` is set.
4. Every beacon change and mandate setting is within range and contract-valid for the built-in operator.
5. It has zero errors. Warnings are allowed, **except** uncovered upkeep, which requires `options.allow_dormant_beacons = true`, per the log.
6. Its content hash (`plan_hash`) is recorded, together with `snapshot_id`, `rules_hash` and `verifier_version`.

### 2.2 What the verifier may and may not simulate ("no dry runs", made precise)

**Allowed.** These are estimators over the seat's own knowledge, and use no simulation of other agents.
- **Pathfinder travel estimates** over the seat's *known* terrain in the snapshot. Unknown or fogged voxels use the last-known state, or "passable at cost ×1.5" if never seen. Results are `eta_ms` as `{optimistic, typical}` per move.
- **Interface-time arithmetic** from `interface_costs`.
- **Quartermaster ledger projection.** The same deterministic Quartermaster code, run on the economy forecast: BMI plus known pending awards and shames, upkeep, $ reserved by queued build targets and structures, and recycle refunds. Arithmetic only.
- **Placement legality** of `place_beacon` and build targets against known terrain and the current sphere geometry.
- **Selector resolution preview** against the snapshot, labelled "if resolved now".
- **Coverage query** for radio (MastOnly geometry).

**Forbidden.**
- stepping the simulation
- forking state
- running operators, combat, unit AI, capture timers or construction progress
- modelling enemy behaviour
- evaluating handler conditions over a projected future

The timeline is a *schedule estimate*, not a prediction of outcomes. The verifier never says "this attack will succeed".

**Why this is fair.** Humans and LLMs get identical estimators. The built-in planner (§5) is bound to the same list, so the "Hard AI rollouts" idea from the research notes is dropped (§8).

### 2.3 Pipeline

```
verify(plan_json_or_bytes, snapshot_id, opts{level: QUICK|FULL}) -> VerifyReport
  S0 decode     strict pbjson/prost parse; size ≤ 256 KiB; UTF-8; unknown field/enum → E0001/E0003
  S1 structure  limits, label uniqueness, forward-only flow, repeat/timeout rules, required on_death/fallback
  S2 resolve    ids/tags/waypoints exist in snapshot/plan; selector legality; capability gating (requires)
  S3 semantic   mandate contract ranges; change legality per beacon state; placement legality;
                interface sequencing (same beacon twice with contradictory patches) ; recycle safety
  S4 estimate   schedule walk (route only, then per-handler worst-case detour), economy projection, coverage
  S5 lint       shadowed/conflicting handlers, dead steps, stale knowledge, style
  → report sorted by (severity desc, path asc, code asc); report_hash = xxh3(canonical report bytes)
```

- `QUICK` runs S0–S3. It is under 5 ms, and the editor calls it on every edit.
- `FULL` adds S4–S5. It is typically under 50 ms, because pathfinding estimates are cached per `(snapshot_id, from, to)`.
- Submit always runs `FULL`.

### 2.4 Determinism of verification

- **Inputs are content-addressed.**
  - `snapshot_id` = hash of the seat's knowledge store frozen at pause start (immutable for the whole pause).
  - `rules_hash`, and `verifier_version` (the semver of the core crate).
- **Pure function.** No wall clock, no `HashMap` iteration (BTreeMap or sorted Vec only), integer maths, and a stable diagnostic sort order.
- **The pathfinder estimator** uses the same integer A* on a known-terrain grid, with deterministic tie-breaking. Its cache is only an optimisation; the key includes the snapshot.
- **Guarantee:** `verify(p, s)` on the LLM's pre-check equals `verify(p, s)` at submit, byte for byte (`report_hash` equal). The submit response echoes `report_hash`, so a client can confirm it.
- **Stale snapshots.** Snapshots belong to a pause. A plan authored against pause *k−1* and submitted in pause *k* is re-verified against snapshot *k*. S2 references may change; the report says so with I0101 "authored against older snapshot".
- **CI.** Golden tests hold 200 plans × snapshots → expected report hashes, checked on 3 operating systems (ties into spike G4).

### 2.5 Diagnostic format

The format is modelled on rustc's JSON diagnostics: a primary span, secondary labels, and suggestions with applicability ([rustc JSON output](https://doc.rust-lang.org/rustc/json.html)). Paths use **JSON Pointer** (RFC 6901) into the canonical JSON of the plan, so an LLM can patch it and the editor can focus the widget.

```json
{
  "code": "E0412",
  "severity": "error",
  "title": "Too many changes in one visit",
  "message": "Interface at step 'fortify_north' has 8 changes; the limit is 6.",
  "path": "/declarative/route/3/interface/changes",
  "related": [
    {"path": "/declarative/route/3/interface/beacon", "label": "beacon b_07 'North Gate'"}
  ],
  "map_refs": [{"beacon_id": "b_07"}],
  "estimate": {"interface_ms": 21500},
  "suggestions": [
    {
      "title": "Split into two visits (changes 1-6, then 7-8)",
      "applicability": "machine_applicable",
      "patch": [
        {"op": "copy", "from": "/declarative/route/3", "path": "/declarative/route/4"},
        {"op": "replace", "path": "/declarative/route/4/label", "value": "fortify_north_2"}
      ]
    }
  ],
  "doc": "game://docs/diagnostics/E0412",
  "plain": "Your commander can make at most 6 changes per beacon visit. Split this visit in two."
}
```

- `patch` is RFC 6902 JSON Patch. `applicability` is `machine_applicable`, `maybe_incorrect` or `has_placeholders`, as in rustc.
  - The editor shows a "Fix" button only for `machine_applicable`.
  - An LLM may apply any suggestion, but sees the applicability.
- `plain` is the beginner sentence the editor shows inline.
- `message` is the precise text for experts and LLMs.
- Report envelope fields:
  - `ok` (true when there are no errors)
  - `qualifies` (`ok`, and the upkeep gate is satisfied)
  - `counts {error, warning, info}`
  - `diagnostics[]`, truncated at 50 with `truncated: true` and a cursor
  - `timeline` (§2.7)
  - `economy` (§2.7)
  - `plan_hash`, `snapshot_id`, `rules_hash`, `verifier_version`, `report_hash`

### 2.6 Diagnostic catalogue (v1 initial set)

| Code | Sev | Check |
|---|---|---|
| E0001 | error | JSON/proto decode failure (includes line and column in `message`) |
| E0003 | error | Unknown field or enum value for the supported schema version |
| E0101 | error | Limit exceeded: steps > 64, expanded > 256, handlers > 12, body > 8, flags > 16 |
| E0110 | error | Missing `on_death` |
| E0111 | error | Missing `fallback` |
| E0120 | error | Duplicate or invalid label |
| E0202 | error | Condition too deep (> 4) or too large (> 24 nodes) |
| E0205 | error | Enemy predicate without explicit `freshness` |
| E0310 | error | Backward jump (branch, GOTO, on_fail, respawn, or resume target before source) |
| E0311 | error | Jump target label not found |
| E0320 | error | `wait_until` without `timeout_ms` |
| E0321 | error | Loop iteration with no time-consuming step |
| E0322 | error | Repeat count > 8 or nesting > 2 |
| E0330 | error | Handler `cooldown_ms` < 5000 or `max_fires` outside 1..8 |
| E0401 | error | Beacon id or tag not found in seat knowledge |
| E0402 | error | Selector with no possible match *and* `none_policy` unset |
| E0410 | error | Interface target is not an own beacon (enemy, neutral, destroyed at snapshot) |
| E0412 | error | More than 6 changes per visit |
| E0420 | error | `place_beacon`: illegal placement (not touching or overlapping own sphere; inside a contested zone; first beacon outside the spawn zone) |
| E0430 | error | Capability structure for a capability that is `NOT_EARNED` and not currently on a ballot |
| E0501 | error | Mandate setting out of range or wrong type for the mandate |
| E0510 | error | Settings patch path invalid for the current or target mandate |
| E0520 | error | `recycle` without `confirm: true` |
| E0601 | error | Uncovered upkeep projected and `allow_dormant_beacons` is false |
| W0602 | warning | Upkeep covered only if a recycle refund or an uncertain award arrives |
| W0603 | warning | Beacon priority `HIGH` on more than half of beacons (the knob loses meaning) |
| W0610 | warning | `broadcast` step or `launch.broadcast_received` while commander or beacon has no radio coverage at the estimated time |
| W0611 | warning | Messaging vocabulary used while the Messaging capability is not `ACTIVE` (the step will be skipped) |
| W0701 | warning | Route typical ETA exceeds the segment (480 s); lists the first step that will not start |
| W0702 | warning | Route plus worst-case handler detours exceed the segment by more than 25% |
| W0703 | warning | Unreachable destination on known terrain (path not found); `maybe` if the route crosses fog |
| W0704 | warning | Interface planned inside a sphere where enemy units were seen within 60 s (interruption risk) |
| W0710 | warning | Handler shadowed: a higher handler's condition is implied by this one's (syntactic implication check on normalised leaves) |
| W0711 | warning | Oscillation risk: two handlers move to each other's trigger area (A flees to X; B fires at X), or a resume sends back into the trigger |
| W0712 | warning | Handler can never fire (condition contradicts `only_in_postures` or constant-false leaves, e.g. `seg_time > 480000`) |
| W0713 | warning | Two visits to the same beacon with contradictory patches (the later visit overrides; is that intended?) |
| W0720 | warning | Target knowledge is stale (last seen > 1 round) |
| W0730 | warning | Recycle leaves units re-homing to a beacon more than 64 voxels away, or recycles the last beacon |
| W0731 | warning | Timed weapon capability expires before or within this segment and the plan builds its structure |
| W0740 | warning | Attack `max_range_from_sphere` target is beyond range (units will stop at the limit) |
| I0002 | info | Duration rounded up to a 50 ms tick multiple |
| I0101 | info | Plan was authored against an older snapshot |
| I0201 | info | Fallback reached at typical ETA t=…; the commander idles for N s (consider more steps) |

Handler-conflict analysis stays *syntactic and conservative*: normalised conditions, interval arithmetic on integer leaves, and area-overlap tests. It never simulates, so it is deterministic and cheap. False negatives are acceptable. False positives must stay rare, so every W07xx carries a `maybe_incorrect` suggestion rather than an auto-fix.

### 2.7 Timeline and economy blocks (shared by editor, LLM and planner)

```json
"timeline": {
  "segment_ms": 480000,
  "route": [
    {"index": 0, "label": "to_north", "kind": "move", "start_ms": 0, "end_ms": 41000,
     "eta": {"optimistic_ms": 36000, "typical_ms": 41000}, "path_id": "p_3f2a", "crosses_fog": false},
    {"index": 1, "label": "fortify_north", "kind": "interface", "start_ms": 41000, "end_ms": 58500,
     "changes": [{"i": 0, "kind": "set_priority", "commit_at_ms": 44500},
                 {"i": 1, "kind": "edit_settings", "commit_at_ms": 47000}]}
  ],
  "fits": true, "slack_ms": 112000, "fallback_from_ms": 368000,
  "handlers": [{"id": "flee", "worst_case_detour_ms": 34000, "max_total_ms": 68000}]
},
"economy": {
  "treasury_now": 1850, "bmi_next": 1000,
  "upkeep_next_round": 900,
  "plan_spend_reserved": 1200, "recycle_refunds": 300,
  "coverage": "ALL_COVERED",
  "beacons": [{"beacon_id": "b_02", "forecast": "COVERED", "priority": "NORMAL"}]
}
```

`path_id` references a polyline that `get_route_preview` returns, so the editor can draw it without a second pathfinder in GDScript.

---

## 3. API / MCP SURFACE for the planning pause

### 3.1 Layering: one API, many clients

```
 Godot client (GDScript editor)  ─┐
 MCP adapter (rmcp; stdio | 127.0.0.1 HTTP) ─┤──►  Seat Gateway (JSON-RPC 2.0 over WebSocket, 127.0.0.1)  ──► core: knowledge store,
 CLI  `gamectl`                   ─┤          auth · scopes · fog filter · rate limits · event bus        verify/estimate/render,
 Built-in planner (in-process)    ─┘          (in-process Rust trait for the planner; same service impl)  plan store, ballots
```

- **Services** are defined in proto (`gp.api.v1`): `SeatService`, `KnowledgeService`, `PlanService`, `BallotService`, `FeedService`, `DocsService`.
- **Gateway methods** are the RPC names in snake_case (`plan.verify`).
- **MCP tools** are thin generated wrappers. Each tool's input schema is sliced from the proto (§1.11) and a hand-written description is added.
- **The Godot editor** calls the gateway methods directly. It sees exactly the same results, diagnostics and render output as an LLM. The editor is "just another client".
- **The planner** calls the same `PlanService` trait in process, with the same scope checks and the same knowledge snapshot.

### 3.2 Phases and timer

Phases: `LOBBY → PLANNING(k) → PLAY(k) → RECAP(k) → PLANNING(k+1) …`.

RECAP is folded into the first seconds of PLANNING, and the recap data stays readable throughout planning.

`get_status` returns:

```json
{"phase":"PLANNING","round":4,"pause_ends_in_ms":241000,"pause_max_ms":300000,
 "seats":[{"seat":"s1","ready":false},{"seat":"s2","ready":true}],
 "my_seat":"s2","submitted":{"plan_hash":"9c1e…","qualifies":true,"at_ms_into_pause":52000},
 "snapshot_id":"k4:ab12…","ballot_open":false,"unread":{"reports":7,"feed":0}}
```

- **Ready status is visible to other seats.** The log wants early resume when everyone is ready. That leaks "they are done", which is a mild information leak; see OQ-9.
- **Submission and readiness are separate.**
  - `submit_plan` stores the plan and does not ready up.
  - `set_ready(true)` requires a qualifying stored plan, or `accept_safe_plan: true`.
  - A seat can resubmit until the pause ends. The last qualifying submission wins.
- **Pause end.**
  - If no qualifying plan was submitted this pause: the last valid beacon configurations are kept (they are world state anyway), and a planner safe plan is filed. The first timeout per match carries no shame, per the log.
  - If a qualifying plan was submitted but the seat is not ready, the plan is used. It is not a timeout.

### 3.3 MCP tool list (27 tools)

Naming is `verb_noun`, snake_case, unprefixed. The server name `voxgame` namespaces them. Annotations follow the MCP tool-annotation hints ([MCP tools spec](https://modelcontextprotocol.io/specification/2025-06-18/server/tools), [annotations post](https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/)).

| # | Tool | Coarse or fine | Scope | Hints | Purpose |
|---|---|---|---|---|---|
| 1 | `get_status` | coarse | observe | readOnly | Phase, timer, ready states, submission, unread counts |
| 2 | `get_briefing` | **coarse** | observe | readOnly | One-call planning digest: recap, economy, threats, beacons, capability deltas, ballot. `detail: brief(≤1.5k tok) \| standard(≤4k) \| full(≤10k)` |
| 3 | `get_recap` | coarse | observe | readOnly | Last segment: key events, awards and shames with $, losses, captures, low-funds alerts |
| 4 | `list_beacons` | fine | observe | readOnly | Own beacons: id, tags, pos, mandate summary, priority, HP, status, units by role, funding forecast. Paginated. |
| 5 | `get_beacon` | fine | observe | readOnly | Full config (mandate settings), structures, recent reports, interface anchor, sphere geometry |
| 6 | `get_map_summary` | coarse | observe | readOnly | Fog-limited map in text: regions grid (8×8 sectors) with terrain, height band, ownership, known enemy presence, freshness. `sector` for zoom. |
| 7 | `query_area` | fine | observe | readOnly | Known contents of a box or sphere: structures, units last seen, terrain height stats, placement legality |
| 8 | `list_known_enemies` | fine | observe | readOnly | Known enemy beacons and structures, unit clusters, with `last_seen_ms` and confidence. Paginated. |
| 9 | `get_reports` | fine | observe | readOnly | Operator reports since cursor; filter by beacon or kind |
| 10 | `get_economy_forecast` | coarse | observe | readOnly | Treasury, BMI next, upkeep per beacon, Quartermaster fill order preview, what-if: `recycle[]`, `priority_changes[]` |
| 11 | `get_capabilities` | coarse | observe | readOnly | Earned, active, suspended, rounds left, known triggers, structure blueprints and costs |
| 12 | `estimate_route` | fine | observe | readOnly | ETA optimistic and typical from A to B on known terrain, and `path_id` |
| 13 | `get_schema` | coarse | docs | readOnly | JSON Schema slice: `part: gameplan \| step \| condition \| change \| mandate:<type>`, `for_seat: bool` drops unusable vocabulary |
| 14 | `list_templates` | coarse | docs | readOnly | Templates with parameter descriptions and difficulty tags |
| 15 | `instantiate_template` | coarse | plan | readOnly | Template id + params → full plan JSON + verify report (does not store) |
| 16 | `verify_plan` | **coarse** | plan | readOnly, idempotent | Full report (§2.5, §2.7). `detail: errors_only \| standard \| full` |
| 17 | `render_plan` | fine | plan | readOnly | Plain-language rendering (the same text as the editor), optional `locale` |
| 18 | `patch_plan` | fine | plan | readOnly | Apply RFC 6902 patch or a suggestion id to a plan and re-verify. Returns the new plan and report; stateless. |
| 19 | `save_draft` | fine | plan | idempotent | Store a named draft in the seat's private plan library (not submitted) |
| 20 | `list_drafts` / `get_draft` | fine | plan | readOnly | Library access (one tool with `id?`) |
| 21 | `submit_plan` | **coarse** | plan.submit | idempotent (by plan_hash) | Verify FULL + store as this pause's submission. Returns report + `accepted`. |
| 22 | `set_ready` | fine | plan.submit | idempotent | Ready or unready. Fails with `NO_QUALIFYING_PLAN` unless `accept_safe_plan`. |
| 23 | `get_safe_plan` | fine | plan | readOnly | What the planner would file on timeout (lets the author compare) |
| 24 | `get_ballot` | coarse | vote | readOnly | Open ballot (4+ seats only): options, deadline, `my_vote` |
| 25 | `cast_vote` | fine | vote | idempotent | Vote or change vote until deadline |
| 26 | `wait_for` | coarse | observe | readOnly | Long-poll until an event: `phase_change \| pause_ends_in<ms \| feed_digest \| ballot_open`, max 60 s wall per call |
| 27 | `get_segment_feed` | coarse | observe | readOnly | During PLAY: event stream page since cursor, plus a digest per 60 s of game time |

**Resources** (read on demand, so they don't occupy context every turn):
- `voxgame://docs/llms.txt`
- `voxgame://docs/diagnostics/{code}`
- `voxgame://rules/interface-costs`
- `voxgame://rules/mandates/{type}`
- `voxgame://templates/{id}`
- `voxgame://examples/{name}`
- `voxgame://schema/gameplan.json`

**Prompts:** `plan_turn` (a scaffold that walks briefing → draft → verify → submit) and `explain_diagnostic`.

**Coarse vs fine.** Three tools do 80% of the work: `get_briefing`, `verify_plan` and `submit_plan`. The fine tools exist for targeted follow-ups. This follows the log's guidance of about 20–30 coarse tools, and the research finding that agents under-monitor state they must explicitly query ([CivBench](https://arxiv.org/abs/2609.02459)).

**Status footer.** Every tool result carries a tiny `_status` footer (`phase`, `pause_ends_in_ms`, `unread`, `submitted_qualifies`). The agent sees the clock without asking. This is the "push digest" decision from the log, adapted to request/response MCP clients that may ignore notifications.

### 3.4 Token budgets, pagination, output format

- **Budgets.** Each read tool takes `detail` (`brief`, `standard`, `full`) with hard token ceilings. Ceilings are measured with a fixed tokenizer-agnostic estimator (4 characters ≈ 1 token) and enforced by truncating lowest-salience items first.
  - `brief`: 1,500 tokens
  - `standard`: 4,000 tokens
  - `full`: 10,000 tokens, which stays well under the Claude Code 25k tool-output default noted in [Anthropic's tool-writing guidance](https://www.anthropic.com/engineering/writing-tools-for-agents)
- **Salience order** is fixed and documented: threats to own beacons → funding risks → capability changes → reports → map detail.
- **Content shape.** Each result has:
  - `structuredContent`: canonical JSON, with an `outputSchema` published
  - a `content` text block: deterministic template prose that summarises the same data (not model-written; this follows the straw man's template-L2 idea)

  Clients that only read text still get the meaning. Identifiers appear in both, for example "North Gate [b_07]".
- **Pagination.** Opaque `cursor` in, `next_cursor` out, the same pattern as MCP's own list pagination. Page sizes: `limit` default 20, max 100. Cursors embed `snapshot_id`, so a cursor from an old snapshot fails with `STALE_CURSOR`.
- **Stable ordering.** Beacons by id, reports by `(seg_ms, beacon_id, seq)`, enemies by `last_seen desc, id`.

### 3.5 Error shapes

There are two levels, per MCP:
- **Protocol errors** (JSON-RPC `error`): unknown tool, malformed arguments against the input schema, auth failure.
- **Tool errors**: `isError: true` with `structuredContent.error`. Output-schema validation is skipped for error results.

```json
{"isError": true,
 "structuredContent": {"error": {
   "code": "PHASE_CLOSED",
   "message": "Planning pause 4 ended 3 s ago; submissions are closed.",
   "retryable": false,
   "hint": {"next_phase": "PLAY", "wait_tool": "wait_for", "args": {"event": "phase_change"}},
   "doc": "voxgame://docs/errors/PHASE_CLOSED"}},
 "content": [{"type":"text","text":"Planning pause 4 ended 3 s ago; submissions are closed. Call wait_for(phase_change)."}]}
```

**Error codes** (closed enum; new values only in minor versions):
- `UNAUTHENTICATED`, `SCOPE_DENIED`
- `PHASE_CLOSED`, `NOT_IN_PHASE`
- `RATE_LIMITED` (with `retry_after_ms`)
- `STALE_SNAPSHOT`, `STALE_CURSOR`
- `NOT_FOUND`
- `PLAN_TOO_LARGE`
- `NO_QUALIFYING_PLAN`
- `BALLOT_NOT_OPEN`
- `VOTING_DISABLED` (fewer than 4 seats)
- `INTERNAL`

Plan *validity* problems are never tool errors. `verify_plan` and `submit_plan` succeed and return `qualifies: false` with diagnostics, so the agent always gets the full report.

### 3.6 Voting

- **Availability.** Voting happens only when `seats ≥ 4`; below that, `get_ballot` returns `VOTING_DISABLED`.
  - Since v1 hosts 1–4 seats, voting exists **only in 4-seat matches** (§8).
- **Ballots.** Ballots open at pause start and close at `pause_end − 15 s`, so the result can feed the snapshot of the *next* round.
  - Alternatively they can resolve before the plan deadline, so an awarded capability is plannable. See OQ-10; the recommended default is to close at 60 s into the pause and apply the result to a refreshed snapshot `k'`. Plans already verified against `k` get I0101 and re-verify automatically.
- **Casting.** `cast_vote {ballot_id, option_id}` can be changed until close. Votes are secret until close; the tally is published in the recap. A tie means the vote fails, per the log.
- **Pity vote.** The pity vote is just a ballot type (`kind: PITY`) with the option `grant_last_place`.

### 3.7 Segment report stream (during PLAY)

**Sources.** The gateway's event bus emits `FeedEvent`s that are **fog-filtered for the seat**. Event kinds include:
- `step_started`, `step_completed`, `step_failed {reason}`
- `handler_fired`, `interface_interrupted`, `change_committed`
- `commander_down`, `commander_respawned`
- `beacon_under_attack`, `beacon_dormant`, `beacon_captured`, `beacon_destroyed`
- `structure_completed`, `capability_state_changed`
- `enemy_sighted` (cluster), `broadcast_sent`, `broadcast_received`
- `award_pending`, `shame_pending` (finalised at segment end)

**Consumption**
- MCP: `get_segment_feed {cursor, detail}` returns events plus a rolling `digest` for each 60 s of game time, as 3–6 template lines.
- `wait_for {event: feed_digest}` blocks until the next digest.
- Optional MCP `notifications/message` pushes digests for clients that surface them.
- **LLMs cannot act during PLAY.** The feed is for learning and commentary. The `plan` scope is inert during PLAY (`NOT_IN_PHASE`).
- **Godot** subscribes to the same bus over WebSocket for the live event timeline.
- **Speed controls.** If all seats agree, 2–4× or skip, per the log. Feed timestamps are in game ms, so digests stay aligned.

### 3.8 Security for local agents

- **Bind address.** The gateway and the MCP HTTP transport listen on `127.0.0.1` and `[::1]` only; binding 0.0.0.0 is not configurable in v1.
- **Host and Origin checks.** Validate `Host` and `Origin` on every HTTP and WebSocket request, and reject with 403 otherwise. This defends against DNS rebinding and follows the MCP transport security requirements ([MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)).
- **Preferred transport.** stdio (`gamectl mcp --seat s2`), which inherits OS user isolation and needs no port for the MCP hop.
- **Tokens.**
  - The host issues a **per-seat controller token** in the lobby: 256-bit random, shown once as a copyable string or QR, and written to `%APPDATA%/<game>/tokens/<match>/<seat>.token` with a user-only ACL.
  - Tokens are bound to `match_id` and `seat` and expire at match end.
  - A token can be revoked from the lobby ("disconnect agent").
- **Scopes:**
  - `observe`
  - `plan` (draft, verify, render)
  - `plan.submit`
  - `vote`
  - `docs`
  - `spectate.nofog`: host-only, and only when no human seat is on this machine, or all seats agree. Otherwise a local human could peek; see OQ-12.
  - `admin` (host: pause settings, speed)

  A token has a scope set chosen at issue. Presets: "AI player" = observe + plan + plan.submit + vote + docs; "Advisor" = observe + plan + docs (no submit).
- **Advisor mode.** An LLM advisor can prepare drafts in a human's library, and the human submits. This is the log's copilot idea, in pause-only form.
- **Rate limits** per token: 20 requests per second burst, 600 per minute; `verify_plan` 5 per second.
- **Sandboxing.** Nothing the API accepts can touch the filesystem or network. Plans are data; drafts are stored in the game's own directory.
- **Audit.** Each seat has an audit log (tool, time, plan_hash), shown in the post-match screen with an "agent" label.

### 3.9 Documentation for coding agents

The `llms.txt` format follows [llmstxt.org](https://llmstxt.org/). The same files are served as resources and shipped in the install directory `docs/`:
- `llms.txt`: H1, a one-paragraph summary, then link sections for Quickstart (5-step planning loop), Schema, Conditions, Beacon changes and interface costs, Mandates, Diagnostics, Templates, Examples, Security.
- `llms-full.txt`: everything concatenated, about 40k tokens, generated in CI from the proto `(gp.doc)` options and markdown pages.
- `docs/examples/*.json`: the §6 plans plus 10 more, each with its verify report, and golden-tested.
- `docs/agent-guide.md`: "How to plan a turn in under 20 tool calls", common mistakes mapped to diagnostic codes, and the budget strategy (`brief` first).
- `gamectl verify plan.json --snapshot latest` and `gamectl schema --part condition`: CLI equivalents, so coding agents can work from a shell too.

Every tool description ends with one example call. Docs for tools and the schema are *generated*, so they cannot drift.

---

## 4. HUMAN EDITOR UX (Godot 4.7 native)

### 4.1 Precedents and what we take from each

| Precedent | Lesson taken |
|---|---|
| Frozen Synapse ([Game Developer](https://www.gamedeveloper.com/business/frozen-synapse-prime---our-recreation-and-some-of-the-challenges)) | Waypoints with commands attached on the map, and precise wait times. **Not taken:** its outcome simulation, because we have no dry runs. |
| Door Kickers 2 go codes ([Steam discussion](https://steamcommunity.com/app/1239080/discussions/0/594015574338601221/)) | Synchronisation by a named signal attached to a path node. Here that is Attack `launch` on segment time or a `GO` broadcast. The step chip shows a go-code badge "A". Also note that DK2 codes fire in path order, which confuses players, so our labels are unique and explicit. |
| FF12 gambits ([wiki](https://finalfantasy.fandom.com/wiki/Gambits)) | Ordered list, first true wins, and a sentence-like "Target: condition → action" row. Small, learnable slot count (12). Unlocking vocabulary maps to capability-gated predicates. |
| Gladiabots ([wiki](https://wiki.gladiabots.com/index.php?title=BotProgramming_Basics)) | Conditions as queries that pass or fail. Sub-AIs as reusable chunks, costed by size, which maps to *handler snippets* that count against limits when inserted. |
| Into the Breach ([GDC postmortem](https://ubm-twvideo01.s3.amazonaws.com/o1/vault/gdc2019/presentations/Into%20the%20Breach%20Postmortem%20Final.pdf)) | Telegraph everything the *system* knows deterministically: travel bars, interface bars, commit ticks, upkeep coverage. Honest about what it doesn't know (fog-hatched segments). |
| Factorio blueprint strings ([wiki](https://wiki.factorio.com/Blueprint_string_format)) | Copy-paste share string `GP1` + base64url(deflate(canonical proto bytes)) for your own library and offline sharing of templates. Never auto-shared in match. |
| Desynced behaviour editor ([wiki](https://wiki.desyncedgame.com/Behavior_Programming)) | Parameters and registers as named slots, which map to plan `waypoints`, `beacon_tags` and `flags`. A cautionary tale for node graphs being heavy for beginners, so v1 uses **lists, not graphs**. |
| Opus Magnum ([histograms](https://steamcommunity.com/app/558990/discussions/0/2381701715715906520/)) | Show a few objective metrics after the fact (segment time used, $ spent, visits completed, interrupts) as a personal post-segment "plan scorecard", with history. Metrics are shown, never merged into one score. |

### 4.2 Screen layout (planning pause)

```
┌────────────────────────────────────────────────────────────────────────────────────┐
│ Round 4 · Planning  ⏱ 4:01  [Ready ☐]   Verify: ● 0 errors ▲ 2 warnings   Submit ▶  │
├───────────────────────────────┬────────────────────────────────────────────────────┤
│  MAP (fog-limited, 3D/top)    │  RULES (gambits)                     [+ Rule ▾]    │
│  • own beacons w/ spheres     │  1 ⠿ ☑ When I'm hurt (HP≤40% & hit in 3s)          │
│  • numbered route chips ①②③   │        → go to safest beacon, wait 8s, continue    │
│  • route polylines + ETA      │  2 ⠿ ☑ When a core beacon is under attack          │
│  • last-known enemies (ghost) │        → visit it: ROE engage, priority HIGH ▲W0704 │
│  • radio coverage overlay     │  3 ⠿ ☐ (disabled) When upkeep at risk …            │
├───────────────────────────────┴────────────────────────────────────────────────────┤
│ SEGMENT CLOCK  0:00 ────────────────────────────────────────────────────────── 8:00 │
│ route  ▭move 0:41 ▮▮iface 0:17 ▭move 1:05 ▮place 0:12 ▭▭▭ … │fallback▒▒▒▒│ slack 1:52│
│ handlers worst-case detour: flee +0:34 ×2   rescue +1:10 ×2    ⚠ may overrun by 0:20 │
├────────────────────────────────────────────────────────────────────────────────────┤
│ STEP INSPECTOR (selected ③ "fortify_north")   Plain: "Go to North Gate and …"      │
│ Beacon: [North Gate b_07 ▾]  Changes (commit order, drag):  1 Priority→HIGH 1.5s … │
└────────────────────────────────────────────────────────────────────────────────────┘
```

**Godot nodes**
- `SubViewport` holds the 3D map.
- `Tree` or `ItemList`-style custom `VBoxContainer` rows hold rules and steps, with `_get_drag_data`/`_can_drop_data`/`_drop_data` drag reordering. Godot's built-in `Tree` drag support has known rough edges ([proposal #3201](https://github.com/godotengine/godot-proposals/issues/3201)), so rows are custom `PanelContainer`s in a `VBoxContainer`.
- The segment clock is a custom `Control` with `_draw()`.
- The inspector uses generated forms (§4.6).

### 4.3 Map timeline interaction

**Building the route**
- **Click an own beacon.** A radial menu appears: *Visit & change…*, *Go here*, *Recycle…*, *Tag as…*. Choosing *Visit & change* appends a `move` step (auto-inserted, labelled "to <beacon>") plus an `interface` step, and opens the inspector.
- **Click the ground.** *Go here* adds a move with a voxel target; *Place beacon here* shows a live legality preview: a green or red ghost sphere and a tooltip giving the reason code (E0420).
- **Click a known enemy beacon.** *Target with Attack beacon…* picks one of your Attack beacons and creates a visit with `edit_settings attack.target`.
- **Selector instead of a fixed target.** Hold Alt when clicking, or use "Make this flexible" in the inspector. This converts a fixed reference into a selector, with presets "whichever of my beacons is weakest / most threatened / nearest".

**Route preview**
- After each edit (debounced 150 ms) the editor calls `plan.verify QUICK` and `estimate_route` for changed legs.
- Paths draw as polylines with ETA labels, for example "0:41 (0:36–0:41)". Legs crossing fog are dashed with a "?" badge.

**Interface-time bars**
- In the segment clock, each interface is a solid bar with commit ticks, one per change.
- Hovering a tick names the change ("Priority→HIGH commits at 0:44").
- Changes can be reordered by drag in the inspector, and the ticks move live. This teaches "most important change first".

**Segment clock**
- The 8:00 track shows move (light), interface (dark), place (dark hatched), wait (outline: min at timeout-uncertain), fallback (grey), and slack.
- A red overflow region past 8:00 shows what will not happen (W0701).
- A second lane shows handler worst-case detours as ghost blocks with fire counts.
- "Fits in 8 minutes" is a **pill**: green if typical ETA fits with ≥ 30 s slack, amber if it fits only optimistically, red if it doesn't fit.

**Scrubbing the clock** moves a ghost commander along the *estimated* route (schedule only, with no enemies or outcomes), labelled "schedule preview". This is allowed by §2.2.

### 4.4 Gambit rule list

**Each row**
- drag handle, enabled checkbox, priority number
- the plain-language sentence (from `render_plan`), with **chips** for each parameter
- inline diagnostic icons

Clicking a chip opens its picker in place: a number stepper with range, a beacon picker that highlights on the map, or an enum dropdown.

**"+ Rule" menu**
1. *From library* (the standard handlers, §1.7)
2. *Blank rule*: a guided picker, **When** → **Do** → **Then**

**Condition picker**
- Three-level menu: Family (Me, My beacons, Enemies I know about, Money, Capabilities, Radio, Time, Plan) → Predicate → Parameters.
- Combining: "+ and" / "+ or" buttons, with max depth 4 shown as a nesting bar; "not" is a toggle.
- Vocabulary the seat can't use yet is shown greyed with a lock and the reason ("needs Messaging"), rather than hidden. This is the FF12 unlock feel.

**Action picker**
- Choose from: Go to…, Visit & change…, Wait until…, Hold…, Broadcast…, Set flag.
- "Then" offers the resume choices worded plainly: *carry on where I was*, *skip what I was doing*, *jump ahead to…*, *stop the route*.

**Conflict hints** (W0710, W0711) render as a connector line between the two rows, with the `plain` sentence and "Show why".

### 4.5 Plain-language rendering (shared, deterministic)

`render_plan` is implemented in Rust and returns **segments**, not a flat string: `[{text, path?, kind: text|chip|warning}]`. Godot builds chips from segments with `path`, so clicking a word focuses the exact proto field.

The same function produces LLM-facing text. It is template-based per message type and locale files, and deterministic.

Example output:
> **Rule 2.** When *North Gate* or *Core* **is under attack** (in the last *10 s*) → go to it and change: *fire at will*; *money priority: high*. Then *carry on where I was*. At most *2 times*, not more often than every *60 s*.

### 4.6 Templates and parameter filling

A template is a stored `Gameplan` with **holes**:

```json
{"template_id": "expand_and_hold", "version": 3, "difficulty": "beginner",
 "params": [
   {"name": "new_site", "type": "LocationRef", "ui": "map_pick", "doc": "Where to place the new beacon",
    "constraint": "placement_legal"},
   {"name": "home", "type": "BeaconRef", "ui": "beacon_pick", "default": {"own": {"order": "SAFEST"}}},
   {"name": "new_mandate", "type": "enum", "values": ["BUILD", "MINE", "DEFEND"], "default": "MINE"}],
 "plan": { "...": "Gameplan JSON with {\"$param\": \"new_site\"} placeholders" }}
```

**Instantiation** is `instantiate_template(id, params)` in Rust. It substitutes, fills defaults, verifies, and returns a *plain plan*. There is no lingering link except `meta.template_ref {id, version, params}`, which is kept for "re-open in template form".

**Editor wizard**
- One card per parameter.
- Map-pick parameters light up legal voxels. A suggested value (from the planner's utility scorer, §5) is pre-filled with a "why" tooltip.
- "Customise" converts the plan to full editing mode.

**v1 template set** (8):
- Hold & Build (beginner)
- Expand & Mine (beginner)
- Fortify Under Threat
- Economy Reset (recycle + priorities)
- Staged Attack with Go-Time
- Capability Rush (structure)
- Beacon Chain Push
- Safe Plan (the planner's)

Each plan-level template includes the standard `on_death` and `fallback`, plus 2–3 library handlers.

### 4.7 Validation inline

**Where diagnostics show**
- Row-level icons, a field-level red outline, and map markers from `map_refs`.
- A top bar counter; clicking it cycles through issues.
- The `plain` text shows by default, and the `message` text under "details".
- "Fix" appears for `machine_applicable` suggestions. It applies the JSON Patch through `patch_plan`, so the editor has no fix logic of its own.

**Cadence**
- `QUICK` on each edit.
- `FULL` after 600 ms idle and before Submit.
- **Submit is disabled** only for errors, and for E0601 unless the "allow dormant beacons" checkbox is ticked. That checkbox shows the list of beacons that will go dormant.

### 4.8 Exact round-trip to the schema

1. **The editor's model *is* a `Gameplan` JSON dictionary.** It is a canonical JSON object held in GDScript as nested Dictionaries and Arrays. No parallel editor model exists. UI-only state lives in `editor_meta` (collapsed rows, custom colours, the map camera bookmark).
2. **All edits are JSON Patch operations** applied locally, for instant UI, and appended to an undo stack. The undo stack is a list of inverse patches.
3. **Canonicalisation authority is the core.** Every `verify` response returns `canonical_plan` (the normalised JSON). If it differs from the local dictionary other than in key order, the editor adopts it and logs a dev warning. In debug builds a CI test fails on any difference.
4. **Enums as strings.** Ids are strings. Integers are always integers, and GDScript `int` is 64-bit, so there are no float coercions. The editor never writes floats, and its JSON writer rejects floats in debug builds.
5. **Golden test.** Load every example → render in editor (headless GUT test) → export → byte-compare canonical JSON.
6. **Share string and file export** use canonical proto bytes, so hashes match what the LLM gets.

### 4.9 Beginner vs expert

| Layer | Beginner (default for new profiles) | Expert (toggle, or auto after 3 submitted plans) |
|---|---|---|
| Entry | Template wizard, with the planner's suggestion pre-filled | Blank plan or last plan cloned |
| Rules | Library rules only; at most 4 shown; sentences only | All vocabulary, raw JSON panel (read and write, verified live), flags, branch, repeat |
| Targets | Fixed picks on the map | Selectors, offsets, tags |
| Clock | Fit pill + bars | Commit ticks, optimistic/typical toggle, handler worst-case lane |
| Diagnostics | `plain`, Fix buttons | Codes, paths, `message`, `related` |
| Keyboard | Mouse-first | Keyboard: `J/K` move rows, `Ctrl+↑/↓` reorder, `/` command palette ("add visit north gate") |

**Accessibility**
- Every row and chip has an accessible name equal to its rendered sentence. Godot 4.5+ supports screen readers for Control nodes via AccessKit ([Godot 4.5 release](https://godotengine.org/releases/4.5/)); the feature is still marked experimental, so test it.
- Colour is never the only signal: icons plus text for severities, and patterns for timeline bars.
- Fonts scale 100–200%.
- A "reduced motion" setting disables the ghost scrubbing animation.

---

## 5. BUILT-IN PLANNER (a consumer of the same API)

### 5.1 Contract

- Crate `planner`, which depends only on the `gp.api.v1` service traits: `KnowledgeService`, `PlanService`.
- It gets a seat token with the *AI player* preset, and receives the same snapshot and briefing as any client. It is forbidden from the sim crate's internal types; this is enforced by crate dependency rules in CI.
- Output is a `Gameplan` that must pass `verify` like everyone's. The planner's own test suite asserts `qualifies == true` for 10,000 generated plans over fuzzed snapshots.
- **Deterministic.** The RNG seed is `xxh3(match_seed, seat, round, "planner")`. The search budget is counted in *evaluation units*, not wall time, so a computer opponent is identical across machines and replays.
- It runs in the host process on a worker thread during the pause. It does not block, and its budget is at most 2 seconds on the reference CPU.

### 5.2 Algorithm: templates + utility scoring (+ repair)

1. **Read.** Call `get_briefing(full)`, `list_beacons`, `list_known_enemies`, `get_economy_forecast` and `get_capabilities`.
2. **Situation features** (integers): threat per own beacon (known enemy strength within reach, freshness-decayed), funding risk per beacon, expansion sites (legal placements scored by ore, height and mast potential), capability opportunities, attack opportunities (known weak or dormant enemy beacons within Attack range), commander risk map.
3. **Candidate generation.** For each template T in the difficulty's pool, bind parameters from top-k feature-ranked options (k = 3 Easy, 5 Normal, 8 Hard), and call `instantiate_template`. This yields about 30–200 candidates.
4. **Composition.** Plans are built from *visit goals* rather than whole templates:
   - Each candidate contributes visit goals (beacon + changes + utility).
   - A greedy insertion scheduler builds a route: insert the highest utility-per-second goal at its cheapest position, using `estimate_route` and interface costs, until typical ETA reaches 85% of the segment (Easy 70%, Hard 90%).
5. **Utility** `U = Σ wᵢ·fᵢ − risk`, where:
   - `f_defence`: reduction in threatened-beacon exposure
   - `f_econ`: projected $ next round, including recycle refunds and mine output estimate
   - `f_expand`: sphere area gained
   - `f_capability`: progress × capability value table
   - `f_attack`: expected pressure on a known enemy beacon × freshness
   - `risk`: commander path exposure (known enemy presence along the path) + interruption risk at interface sites

   The weights come from a per-difficulty table plus a *personality* (Turtle, Balanced, Raider, Economist), chosen at match start from the seed.
6. **Handlers.** The standard pack is fixed by difficulty:
   - Easy: flee only
   - Normal: flee, rescue, fix funding
   - Hard: all of those, plus opportunistic retarget and a go-time broadcast when Messaging is active
7. **Repair loop.** Run `verify FULL`. For each error, apply `machine_applicable` suggestions, or drop the offending goal. Repeat at most 4 times. If the plan still doesn't qualify, emit the Safe Plan (§5.4). This must never happen; it is logged as a planner bug.
8. **Mandate settings** come from per-template defaults adjusted by features. For example, Defend `roe` becomes `ENGAGE_ON_SIGHT` if threat > t; Attack `launch.seg_time_at_least` is set to an estimated staging time + 60 s.

### 5.3 Difficulty levels

| | Easy | Normal | Hard |
|---|---|---|---|
| Knowledge use | Ignores last-known enemies older than 1 round | Full seat knowledge | Full, plus freshness-weighted inference of enemy expansion direction (from seen beacons only) |
| Candidates / top-k | 30 / 3 | 100 / 5 | 200 / 8 |
| Route fill | 70% | 85% | 90% |
| Handlers | 1 | 3 | 5 |
| Selectors | Fixed refs only | Some selectors | Selectors everywhere useful |
| Mistake injection | 1 deliberately suboptimal goal, 20% chance | none | none |
| Votes | random among non-hostile | utility | utility + standing-aware |
| Rollouts | none | none | **none**. The research proposed fork rollouts; dropped by "no dry runs" (§2.2, §8). |

**Fairness knobs** are exposed to the host: "planner thinking budget" and "planner sees ghosted last-known info".

### 5.4 Safe plan (filed on timeout or failed verification)

**Goal:** never make things worse; keep the commander alive; fix only emergencies that are *cheap and unambiguous*.

**Algorithm**
1. Keep all beacon configurations as they are. This is the log's "last valid config".
2. Route:
   - `move` to `safest_own_beacon`.
   - If `upkeep_coverage == SOME_AT_RISK`: add at most 2 visits of `set_priority HIGH` to at-risk beacons that are ≤ 60 s away, nearest first. Never recycle, never switch mandates, never place beacons.
   - Then `hold` in fallback (`shadow` the safest beacon).
3. Handlers: flee when hurt; rescue a beacon tagged core (settings: ROE engage only).
4. `on_death`: `CONTINUE`, with respawn steps "move to safest, hold 5 s".
5. `allow_dormant_beacons: true`, because a safe plan must always qualify, and dormancy is then the world's consequence. The recap says so plainly.

**Properties**
- Deterministic.
- Verified in CI to qualify over fuzzed snapshots.
- Shown to the seat via `get_safe_plan` at any time, so humans and LLMs know the price of a timeout.

---

## 6. WORKED EXAMPLES

**Shared situation** (seat `s2`, round 4 of a 4-seat match)
- Own beacons:
  - `b_01` "Core" at (40,12,60), tag `core`: Defend
  - `b_02` "Quarry" (72,10,58): Mine
  - `b_03` "North Gate" (44,14,110): Build
- Treasury $1,850; BMI $1,000; next-round upkeep $900.
- Known enemy beacon `e_11` (seat s3) at (120,16,140), last seen 70 s before the segment ended.
- Messaging: `EARNED_UNBUILT`.

### 6.1 Beginner: template "Expand & Mine" (instantiated)

```json
{
  "schema_version": {"major": 1, "minor": 0},
  "header": {"seat": "s2", "round": 4, "snapshot_id": "k4:ab12", "author_kind": "HUMAN"},
  "meta": {"title": "Expand & Mine east", "template_ref": {"id": "expand_and_mine", "version": 3,
           "params": {"new_site": {"voxel": {"x": 96, "y": 11, "z": 62}}, "new_mandate": "MINE"}}},
  "declarative": {
    "route": [
      {"label": "to_site", "move": {"to": {"voxel": {"x": 96, "y": 11, "z": 62}}, "arrive_radius": 2, "pace": "AVOID_KNOWN_THREATS"},
       "timeout_ms": 120000},
      {"label": "place_east", "place_beacon": {"at": {"voxel": {"x": 96, "y": 11, "z": 62}}, "tags": ["east"],
        "initial": {"priority": "NORMAL", "operator": {"builtin": "DEFAULT"},
          "mandate": {"roe": "RETURN_FIRE", "retreat_hp_pct": 30,
            "mine": {"resource_priority": ["ORE_ANY"], "stop_at_pool": 0, "dig_max_depth": 4,
                     "keep_pillar_every": 6, "flee_on_threat": true}}}}},
      {"label": "back_home", "move": {"to": {"beacon_anchor": {"beacon_id": "b_01"}}, "arrive_radius": 3}}
    ],
    "handlers": [
      {"id": "flee", "label": "Flee when hurt", "enabled": true,
       "when": {"all": {"items": [{"cmdr_hp_pct": {"cmp": "LE", "pct": 40}},
                                  {"cmdr_took_damage_within": {"ms": 3000}}]}},
       "body": [{"label": "flee_move", "move": {"to": {"safest": {}}, "arrive_radius": 3}},
                {"label": "flee_hold", "hold": {"ms": 8000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 3}
    ]
  },
  "on_death": {"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2,
               "respawn_steps": [{"label": "rs_hold", "hold": {"ms": 5000}}]},
  "fallback": {"hold": {"at": {"beacon_anchor": {"safest": {}}}}},
  "options": {"allow_dormant_beacons": false, "reflex_hp_pct": 20}
}
```

**Editor rendering**
> **Plan: Expand & Mine east.** Fits in 8:00 · uses 2:31 · slack 5:29
> ① Go to *(96, 11, 62)*, avoiding known threats (0:52; give up after 2:00).
> ② Place a new beacon there, tag *east*: **Mine** any ore, dig ≤ 4 deep, pillar every 6, flee when threatened; money priority *normal*. (0:12)
> ③ Go back to *Core* (1:27).
> Then: wait at my safest beacon.
> **Rules:** 1. When I'm hurt (HP ≤ 40%, hit in the last 3 s) → go to my safest beacon, wait 8 s, carry on where I was. At most 3 times, every 30 s at most.
> **If I die:** after respawn wait 5 s, then carry on; after 2 deaths, just wait at my safest beacon.
> ⓘ I0201 You'll be idle from about 2:31. Consider adding more visits.
> ✔ Upkeep covered next round ($900 + new beacon $150 of $2,850 forecast).

### 6.2 Intermediate: fortify, fix economy, capability structure

```json
{
  "schema_version": {"major": 1, "minor": 0},
  "header": {"seat": "s2", "round": 4, "snapshot_id": "k4:ab12", "author_kind": "HUMAN"},
  "meta": {"title": "Mast up, gate hardened", "beacon_tags": [{"tag": "core", "beacon_ids": ["b_01"]}]},
  "declarative": {
    "route": [
      {"label": "to_gate", "move": {"to": {"beacon_anchor": {"beacon_id": "b_03"}}, "arrive_radius": 3}},
      {"label": "gate_build", "interface": {"beacon": {"beacon_id": "b_03"}, "changes": [
        {"build_capability_structure": {"capability": "cap.messaging", "structure": "RADIO_MAST",
                                         "anchor": {"x": 46, "y": 22, "z": 114}}},
        {"add_build_target": {"blueprint_id": "wall_reinforced_line_8", "anchor": {"x": 40, "y": 14, "z": 118}, "rotation": 1, "order": 2}},
        {"edit_settings": {"entries": [{"path": "build.repair_threshold_pct", "value": {"int": 75}}]}}]}},
      {"label": "to_quarry", "move": {"to": {"beacon_anchor": {"beacon_id": "b_02"}}, "arrive_radius": 3},
       "skip_if": {"beacon_status_is": {"beacon": {"beacon_id": "b_02"}, "statuses": ["DESTROYED", "CAPTURED"]}}},
      {"label": "quarry_prio", "interface": {"beacon": {"beacon_id": "b_02"}, "changes": [
        {"set_priority": "HIGH"},
        {"edit_settings": {"entries": [{"path": "mine.flee_on_threat", "value": {"bool": true}},
                                       {"path": "retreat_hp_pct", "value": {"int": 40}}]}}]},
       "skip_if": {"beacon_status_is": {"beacon": {"beacon_id": "b_02"}, "statuses": ["DESTROYED", "CAPTURED"]}}},
      {"label": "wait_mast", "wait_until": {"until": {"beacon_structure_count": {"beacon": {"beacon_id": "b_03"},
         "kind": "RADIO_MAST", "cmp": "GE", "n": 1}}, "timeout_ms": 150000, "on_timeout": "SKIP"}},
      {"label": "to_core", "move": {"to": {"beacon_anchor": {"tag": "core"}}, "arrive_radius": 3}},
      {"label": "core_defend", "interface": {"beacon": {"tag": "core"}, "changes": [
        {"edit_settings": {"entries": [{"path": "defend.pursue", "value": {"enum_name": "SPHERE_EDGE"}},
                                       {"path": "defend.auto_fortify", "value": {"bool": true}}]}}]}}
    ],
    "handlers": [
      {"id": "flee", "label": "Flee when hurt", "enabled": true,
       "when": {"all": {"items": [{"cmdr_hp_pct": {"cmp": "LE", "pct": 40}}, {"cmdr_took_damage_within": {"ms": 3000}}]}},
       "body": [{"label": "f1", "move": {"to": {"safest": {}}, "arrive_radius": 3}}, {"label": "f2", "hold": {"ms": 8000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 3},
      {"id": "rescue_core", "label": "Rescue core", "enabled": true,
       "when": {"beacon_under_attack_within": {"beacon": {"tag": "core"}, "ms": 10000}},
       "body": [{"label": "r1", "move": {"to": {"beacon_anchor": {"tag": "core"}}, "arrive_radius": 3}},
                {"label": "r2", "interface": {"beacon": {"tag": "core"}, "changes": [
                  {"edit_settings": {"entries": [{"path": "roe", "value": {"enum_name": "ENGAGE_ON_SIGHT"}}]}},
                  {"set_priority": "HIGH"}]}}],
       "resume": "CONTINUE", "cooldown_ms": 60000, "max_fires": 2},
      {"id": "funds", "label": "Fix funding", "enabled": true,
       "when": {"upkeep_coverage": {"is": "SOME_AT_RISK"}},
       "body": [{"label": "u1", "move": {"to": {"beacon_anchor": {"own": {"order": "LOWEST_FUNDING"}}}, "arrive_radius": 3}},
                {"label": "u2", "interface": {"beacon": {"own": {"order": "LOWEST_FUNDING"}}, "changes": [{"set_priority": "HIGH"}]}}],
       "resume": "CONTINUE", "cooldown_ms": 90000, "max_fires": 1}
    ]
  },
  "on_death": {"on_respawn": "CONTINUE", "max_deaths_before_fallback": 2,
               "respawn_steps": [{"label": "rs", "hold": {"ms": 5000}}]},
  "fallback": {"shadow": {"beacon": {"tag": "core"}, "radius": 8}},
  "options": {"allow_dormant_beacons": false, "reflex_hp_pct": 20}
}
```

**Editor rendering**
> **Plan: Mast up, gate hardened.** Fits · uses 5:48 typical (6:40 if handlers fire max) · ▲ 1 warning
> ① Go to *North Gate* (0:38).
> ② At *North Gate* change (13.5 s): 1) build **Radio Mast** (enables Messaging) at (46, 22, 114); 2) add build target *reinforced wall ×8*, order 2; 3) repair when below *75%*.
> ③ Go to *Quarry*, unless it's destroyed or captured (0:44).
> ④ At *Quarry* change (6.5 s): money priority → **high**; flee on threat *on*; retreat at *40%* HP. (Skipped if Quarry is lost.)
> ⑤ Wait until *North Gate* has a Radio Mast, at most 2:30, then move on anyway.
> ⑥ Go to *Core* (0:51). ⑦ At *Core* change (4 s): chase to *sphere edge*; auto-fortify *on*.
> Then: stay within 8 of *Core*.
> **Rules:** 1. When I'm hurt → safest beacon, wait 8 s, carry on. 2. When *Core* was attacked in the last 10 s → go there: *fire at will*, priority *high*; carry on (≤ 2×, every 60 s). 3. When some beacon's upkeep is at risk → set the poorest beacon's priority *high* (once).
> ▲ W0603 Step ④ and Rule 2 can both set priority HIGH (Quarry and Core); if both happen, 2 of 3 beacons are HIGH and the knob loses meaning. *Fine if intended.*

### 6.3 Expert: synchronised strike with selectors, flags, branch, and a go broadcast

Assumes round 5, with Messaging `ACTIVE` (the mast was built in round 4) and `b_04` "Spear" running an Attack mandate. The route is deliberately short. Most of the adaptation happens in the Attack operator, which was configured on site, and in handlers.

```json
{
  "schema_version": {"major": 1, "minor": 0},
  "header": {"seat": "s2", "round": 5, "snapshot_id": "k5:77e0", "author_kind": "LLM"},
  "meta": {"title": "Spear at 4:00, hold the line",
           "beacon_tags": [{"tag": "core", "beacon_ids": ["b_01"]}, {"tag": "spear", "beacon_ids": ["b_04"]}],
           "waypoints": [{"name": "ridge", "voxel": {"x": 88, "y": 24, "z": 120}}]},
  "declarative": {
    "flags": [{"name": "strike_armed"}, {"name": "aborted"}],
    "route": [
      {"label": "to_spear", "move": {"to": {"beacon_anchor": {"tag": "spear"}}, "arrive_radius": 3}},
      {"label": "arm_spear", "interface": {"beacon": {"tag": "spear"}, "changes": [
        {"edit_settings": {"entries": [
          {"path": "attack.target", "value": {"ref": {"known_enemy_beacon": {"filter": {"last_seen_within_ms": 900000}, "order": "NEAREST"}}}},
          {"path": "attack.retarget", "value": {"enum_name": "MARKED_ONLY"}},
          {"path": "attack.launch", "value": {"launch_any": [{"seg_time_at_least": {"ms": 240000}},
                                                              {"broadcast_received": {"code": "A"}}]}}]}},
        {"edit_settings": {"entries": [
          {"path": "attack.retreat_when", "value": {"retreat_any": [{"losses_pct_at_least": 50}, {"elapsed_ms_since_launch": 120000}]}},
          {"path": "attack.after", "value": {"enum_name": "REGROUP_AT_STAGING"}}]}},
        {"set_priority": "HIGH"}]}},
      {"label": "armed", "set_flag": {"name": "strike_armed"}},
      {"label": "to_ridge", "move": {"to": {"waypoint": "ridge"}, "arrive_radius": 2, "pace": "AVOID_KNOWN_THREATS"},
       "skip_if": {"not": {"in_radio_coverage": {"at": {"waypoint": "ridge"}}}}, "on_fail": "SKIP"},
      {"label": "watch", "wait_until": {"until": {"any": {"items": [
          {"enemy_beacon_known": {"filter": {"max_distance_from": {"waypoint": "ridge"}, "radius": 64}, "freshness": {"visible_now": {}}}},
          {"seg_time": {"cmp": "GE", "ms": 225000}}]}},
        "timeout_ms": 200000, "on_timeout": "SKIP"}},
      {"label": "decide", "branch": {"if": {"enemy_units_near": {"area": {"sphere": {"center": {"waypoint": "ridge"}, "radius": 48}},
          "cmp": "GE", "n": 10, "freshness": {"visible_now": {}}}}, "goto_label": "abort_path"}},
      {"label": "mark", "broadcast": {"mark_target": {"target": {"known_enemy_beacon": {"filter": {"last_seen_within_ms": 60000}, "order": "WEAKEST_HP"}}}},
       "on_fail": "SKIP"},
      {"label": "go", "broadcast": {"go": {"code": "A"}}, "on_fail": "SKIP"},
      {"label": "to_core_after", "move": {"to": {"beacon_anchor": {"tag": "core"}}, "arrive_radius": 3},
       "on_fail": {"goto": "end"}},
      {"label": "abort_path", "checkpoint": {}, "skip_if": {"step_reached": {"label": "go"}}},
      {"label": "abort_flag", "set_flag": {"name": "aborted"}, "skip_if": {"step_reached": {"label": "go"}}},
      {"label": "disarm", "interface": {"beacon": {"tag": "spear"}, "changes": [
         {"set_mandate": {"roe": "RETURN_FIRE", "retreat_hp_pct": 35,
           "defend": {"engagement_radius_pct": 100, "pursue": "SPHERE_EDGE", "auto_fortify": false}}}]},
       "skip_if": {"not": {"flag": {"name": "aborted"}}}},
      {"label": "end", "checkpoint": {}}
    ],
    "handlers": [
      {"id": "flee", "label": "Flee when hurt", "enabled": true, "preempt_handlers": true,
       "when": {"all": {"items": [{"cmdr_hp_pct": {"cmp": "LE", "pct": 45}}, {"cmdr_took_damage_within": {"ms": 3000}}]}},
       "body": [{"label": "f1", "move": {"to": {"safest": {}}, "arrive_radius": 3}}, {"label": "f2", "hold": {"ms": 6000}}],
       "resume": "CONTINUE", "cooldown_ms": 20000, "max_fires": 3},
      {"id": "core_breach", "label": "Core in trouble", "enabled": true,
       "when": {"all": {"items": [
          {"beacon_under_attack_within": {"beacon": {"tag": "core"}, "ms": 8000}},
          {"beacon_hp_pct": {"beacon": {"tag": "core"}, "cmp": "LE", "pct": 60}}]}},
       "body": [{"label": "c1", "move": {"to": {"beacon_anchor": {"tag": "core"}}, "arrive_radius": 3}},
                {"label": "c2", "interface": {"beacon": {"tag": "core"}, "changes": [
                  {"edit_settings": {"entries": [{"path": "roe", "value": {"enum_name": "ENGAGE_ON_SIGHT"}},
                                                 {"path": "defend.pursue", "value": {"enum_name": "NEVER"}}]}},
                  {"set_priority": "HIGH"}]}}],
       "resume": "SKIP_STEP", "cooldown_ms": 60000, "max_fires": 2, "only_in_postures": ["ROUTE", "FALLBACK"]},
      {"id": "late_threat_mark", "label": "Mark raiders near Quarry", "enabled": true,
       "when": {"all": {"items": [{"enemy_units_near": {"area": {"beacon_sphere": {"beacon_id": "b_02"}}, "cmp": "GE", "n": 3, "freshness": {"visible_now": {}}}},
                                  {"in_radio_coverage": {"at": {"commander": {}}}}]}},
       "body": [{"label": "t1", "broadcast": {"threat_at": {"area": {"beacon_sphere": {"beacon_id": "b_02"}}}}, "on_fail": "SKIP"}],
       "resume": "CONTINUE", "cooldown_ms": 45000, "max_fires": 4},
      {"id": "funds", "label": "Fix funding", "enabled": true,
       "when": {"upkeep_coverage": {"is": "SOME_AT_RISK"}},
       "body": [{"label": "u1", "move": {"to": {"beacon_anchor": {"own": {"order": "LOWEST_FUNDING"}}}, "arrive_radius": 3}},
                {"label": "u2", "interface": {"beacon": {"own": {"order": "LOWEST_FUNDING"}}, "changes": [{"set_priority": "HIGH"}]}}],
       "resume": "CONTINUE", "cooldown_ms": 90000, "max_fires": 1}
    ]
  },
  "on_death": {"on_respawn": "GOTO", "goto_label": "abort_path", "max_deaths_before_fallback": 2,
               "respawn_steps": [{"label": "rs1", "move": {"to": {"safest": {}}, "arrive_radius": 3}}, {"label": "rs2", "hold": {"ms": 4000}}]},
  "fallback": {"patrol": {"between": [{"beacon_anchor": {"tag": "core"}}, {"beacon_anchor": {"beacon_id": "b_03"}}], "dwell_ms": 20000}},
  "options": {"allow_dormant_beacons": false, "reflex_hp_pct": 20}
}
```

**Editor rendering**
> **Plan: Spear at 4:00, hold the line.** Fits · 4:20 route (5:55 with worst-case rules) · ▲ 2 warnings
> ① Go to *Spear* (0:47).
> ② At *Spear* change (8 s): 1) attack **the nearest enemy beacon I've seen in the last 15 min**, only switch to *marked* targets, launch **at 4:00 or on go-code A**; 2) retreat at *50% losses* or *2:00 after launch*, then regroup at staging; 3) money priority **high**.
> ③ Note: strike armed.
> ④ Go to *ridge*, avoiding known threats (skipped if the ridge has no radio coverage).
> ⑤ Wait until I see an enemy beacon within 64 of the ridge, or it's 3:45 (at most 3:20).
> ⑥ **If** I see ≥ 10 enemy units near the ridge right now → jump to *abort_path*.
> ⑦ Broadcast: mark the weakest enemy beacon seen in the last minute. ⑧ Broadcast: **go-code A**.
> ⑨ Go to *Core* (on failure jump to *end*).
> ⑩ *abort_path* — (only if ⑧ didn't happen) note: aborted; at *Spear* switch mandate to **Defend** (8 s): return fire, retreat 35%, chase to sphere edge.
> Then: patrol between *Core* and *North Gate*, 20 s at each.
> **Rules:** 1. Hurt (HP ≤ 45%, hit in 3 s) → safest beacon, wait 6 s, carry on *(interrupts other rules)*. 2. Core attacked in the last 8 s **and** Core HP ≤ 60% → go there: *fire at will*, *don't chase*, priority *high*; then skip what I was doing (≤ 2×). 3. ≥ 3 enemies seen in *Quarry* sphere and I have radio → broadcast threat there (≤ 4×, every 45 s). 4. Upkeep at risk → poorest beacon priority *high* (once).
> **If I die:** go to my safest beacon, wait 4 s, then jump to *abort_path*; after 2 deaths, patrol.
> ▲ W0603 3 of 5 beacons could be HIGH (steps ②, rules 2 and 4).
> ▲ W0704 *Spear* interface at 0:47: enemies were seen near Spear 40 s before the pause (interruption risk).

**Semantics notes this example relies on**
- **Forward-only rule for `on_death` and `resume` jumps.** The target must not be before the step where the jump *could* originate. For handler and respawn jumps the origin is dynamic, so the rule is enforced at runtime: if the program counter is already past the target, the jump acts as `CONTINUE`, and the feed reports `jump_ignored_backward`. The verifier checks only that the label exists and emits I0310 as an explanation.
- **Why this survives 8 minutes.**
  - Launch timing lives in the Attack operator (4:00 or go-code), so a dead commander doesn't stall the strike.
  - The abort path is taken only if the commander sees overwhelming force *or* dies, and it converts Spear to Defend.
  - Rule 2 skips the current step rather than resuming a stale leg.
  - Fallback patrol keeps the commander useful after 4:20.

### 6.4 Example MCP session: Claude Code plans round 5 for seat s2

```text
→ get_status {}
← {"phase":"PLANNING","round":5,"pause_ends_in_ms":296000,"snapshot_id":"k5:77e0","ballot_open":true, ...}

→ get_briefing {"detail":"standard"}
← (≈3.1k tokens) RECAP r4: +$1,000 BMI, award "Early Antenna" +$250, no shames. Radio Mast at North Gate [b_03]
  completed 5:12 → Messaging ACTIVE. Quarry [b_02] raided at 6:40 (3 raiders), survived 58% HP.
  ECONOMY: treasury $2,410; upkeep next $1,050 (5 beacons) — ALL_COVERED; Quartermaster fill: b_04 Spear (Attack) short $120 for units.
  THREATS: s3 beacon e_11 (120,16,140) seen 1:10 ago, HP ~70%; 6–9 raiders last seen near ridge (88,24,120) 2:30 ago.
  CAPABILITIES: messaging ACTIVE; vehicle_bay NOT_EARNED (on ballot).
  BALLOT b5: "Grant Vehicle Bay to seat with most captures" | "Pity: +1×BMI to last place (s4)" — closes in 0:56.
  _status: pause 4:56 left, unread reports 12

→ get_beacon {"beacon_id":"b_04","detail":"brief"}
← Spear [b_04] Attack · priority NORMAL · target: none (lost) · staging (70,12,96) · raiders 7 · launch: force≥8 …

→ cast_vote {"ballot_id":"b5","option_id":"pity_s4"}
← {"recorded":true,"closes_in_ms":52000}

→ list_templates {"tag":"attack"}
← [{"id":"staged_attack_go_time","params":[...]} , ...]

→ get_schema {"part":"step","for_seat":true}
← {JSON Schema slice, 2.2k tokens; broadcast unlocked}

→ estimate_route {"from":{"beacon_anchor":{"beacon_id":"b_01"}},"to":{"beacon_anchor":{"beacon_id":"b_04"}}}
← {"optimistic_ms":42000,"typical_ms":47000,"path_id":"p_91","crosses_fog":false}

→ verify_plan {"plan": <draft of §6.3 without handler "funds" and with on_death missing>, "detail":"standard"}
← {"ok":false,"qualifies":false,"counts":{"error":2,"warning":2},
   "diagnostics":[
    {"code":"E0110","path":"/on_death","message":"Plan has no on_death block.",
     "suggestions":[{"title":"Insert default on_death","applicability":"machine_applicable","patch":[{"op":"add","path":"/on_death","value":{...}}]}]},
    {"code":"E0205","path":"/declarative/route/5/branch/if/enemy_units_near","message":"Enemy predicate requires explicit freshness.",
     "suggestions":[{"title":"Use VISIBLE_NOW","applicability":"maybe_incorrect","patch":[...]}]},
    {"code":"W0603", ...}, {"code":"W0704", ...}],
   "timeline":{"fits":true,"slack_ms":220000, ...}, "report_hash":"r:5d0c…"}

→ patch_plan {"plan": <draft>, "patch":[<E0110 fix>, {"op":"add","path":"/on_death/on_respawn","value":"GOTO"}, ... freshness ...]}
← {"plan": {...}, "report": {"ok":true,"qualifies":true,"counts":{"error":0,"warning":2}, "report_hash":"r:a17e…"}}

→ get_safe_plan {}
← (brief) "Go to Core, set Spear priority HIGH (funding), shadow Core." — for comparison

→ render_plan {"plan": {...}}
← (the plain-language text shown in §6.3)

→ submit_plan {"plan": {...}}
← {"accepted":true,"qualifies":true,"plan_hash":"9c1e…","report_hash":"r:a17e…"}   ← identical to pre-verify

→ set_ready {"ready":true}
← {"ready":true,"all_ready":false,"_status":{"pause_ends_in_ms":171000}}

→ wait_for {"event":"phase_change"}         (returns when PLAY starts)
→ wait_for {"event":"feed_digest"}          (repeat during PLAY)
← DIGEST 1:00–2:00: step 'arm_spear' committed 3/3 at 0:58. Enemy cluster (7) seen near ridge 1:44. Rule 'late_threat_mark' fired 1×.
```

Session total: 16 tool calls and about 14k tokens in results, well inside a 5-minute pause.

---

## 7. OPEN QUESTIONS for the owner (with recommended defaults)

| # | Question | Options | Recommended default |
|---|---|---|---|
| OQ-1 | Sim tick rate (unstated in the log) | 20 Hz · 30 Hz · 60 Hz | **20 Hz.** The plan decision tick is 250 ms. Cheapest for the G3 300-unit budget; the schema is in ms, so it can change later. |
| OQ-2 | Interface cost numbers | Log's rough values (3 / 8 / 10–30 s) · table in §1.5 · faster (×0.5) | **§1.5 table** (handshake 1.5 s; knob 1.5 s; settings 2 s + 0.5 s per extra field; mandate 8 s; recycle 10 s; beacon placement 12 s). Tune in playtests. |
| OQ-3 | Partial commit on interrupted multi-change visits | All-or-nothing visit · per-change commit in order | **Per-change commit.** Teaches ordering; keeps the log's "old config kept" for the interrupted change. |
| OQ-4 | Can a gameplan send messages? | Never in v1 · only after Messaging is active and in mast coverage · always via commander | **Only after Messaging is `ACTIVE` and within mast coverage.** Operators also start sending then. |
| OQ-5 | Recycle timing: log says "between rounds, instant refund"; brief lists recycle as a beacon change | Pause-time action without commander (breaks on-site rule) · on-site interface during play, refund at commit · both | **On-site interface during play** (10 s), refund at commit. Keeps the lore rule; the Quartermaster projects the refund. |
| OQ-6 | Attack scope outside its sphere | Unlimited · rules cap (e.g. 96 voxels) · must stay within own or contested spheres | **Rules cap `max_range_from_sphere` ≤ 96 voxels**, settable lower. |
| OQ-7 | Plan when nothing qualifying is submitted | Safe plan (log) · reuse last round's plan re-verified · idle | **Safe plan** (log), but the editor offers "resubmit last round's plan" as a one-click action during the pause. |
| OQ-8 | Beginner limit exposure | Show all 12 handler slots · unlock slots with experience · 4 then unlock | **Show 4 in beginner mode, 12 in expert.** No gameplay gating; the same schema limits apply to all seats. |
| OQ-9 | Ready-status leakage | Show others' ready state · show only "N of M ready" · hide until all ready | **"N of M ready"** without identities. |
| OQ-10 | Ballot close time vs plan deadline | Close at pause end (result next round) · close at 60 s into pause and refresh snapshot | **Close at 60 s**, refresh to snapshot k′, auto re-verify stored drafts. Voted capabilities become plannable now. |
| OQ-11 | Voting at 4 seats only (v1 max is 4) | Keep 4+ · allow 3+ · allow spectator/AI-referee votes | **Allow 3+ in v1.** Otherwise voting is almost never exercised. Ties still fail. |
| OQ-12 | No-fog spectator camera on a host with a human seat | Allow · disable while any local human seat plays · require all-seat consent | **Disable while a human seat on this machine is in PLANNING**; allow during PLAY (no manual control, so no advantage). |
| OQ-13 | Hard planner lookahead | Fork rollouts (research) · none (no dry runs) · rollouts on seat knowledge only | **None.** Hard differs by search breadth, selectors and handlers (§5.3). |
| OQ-14 | Max changes per visit | 4 · 6 · unlimited with cost | **6**. |
| OQ-15 | JSON Schema generation path | Buf plugin · schemars · proto + in-house descriptor generator | **Proto + in-house generator** (§1.11), with the Buf plugin as a day-1 fallback and test oracle. |

---

## 8. Contradictions and stale items found in the decisions log (and how this design resolves them)

1. **Default AI as WASM** (§2a "Default AI implemented as a WASM operator"; §2b "operators are WASM only"; G5 in Phase 0) vs **§2.0** "v1 ships built-in operators only; wasmi moves out of v1". *Resolved:* native Rust built-in operators; `OperatorRef.builtin` only; WASM field numbers reserved.
2. **§2a "qualifying" definition** (ABI, imports, fuel dry run, content hash) assumes WASM uploads, and includes a *sandboxed dry run*. This conflicts with **"No dry runs"** and with v1's data-only plans. *Resolved:* §2.1 redefines qualifying; §2.2 lists what may be estimated.
3. **§2a "LLM uploads per-beacon operator script changes"** vs built-in only. *Resolved:* LLMs change beacons only via on-site interface changes in the plan.
4. **Messaging with no sender.** Default operators are "receive-only until messaging is earned", but nothing else sends in v1. *Resolved:* no sender exists until Messaging is `ACTIVE`; then the commander can broadcast via mast coverage and operators send.
5. **Radio mast status.** §2.0 "Mast only" radio versus the capability list, where Radio Mast is a capability structure. Is the mast the *Messaging* capability structure, or a basic building? *Assumed:* the mast is the Messaging structure.
6. **Research "Horizon" connectivity recommended for v1** vs §2.0 "Mast only". §2.0 wins; the extension seams are in §1.9.
7. **Control points.** §2 says "Changing a mandate costs control points", and a later bullet says "No control points; cost is physical". The later bullet wins.
8. **Mandate draft fields** `budget`, `priority`, `override policy` (§3) vs the Quartermaster (no envelopes, one priority knob per beacon) and "no manual control". *Resolved* in §1.6: budget and override removed; priority moved to the beacon.
9. **Mine `deliver_to: core, storage or logistics link`** (§3) vs the economy decision "mining drones physically deliver to beacon storage". *Resolved:* beacon storage only; field reserved.
10. **Claims and protection.** §1 says "no protections, everything attackable" vs §4.D "non-destructible to others by default". §1 is later and decided.
11. **Recycling "between rounds, instant refund"** vs "all beacon changes need the commander on site". This is OQ-5.
12. **Research planner "bounded fork rollouts for Hard"** vs "no dry runs" and fairness. Dropped (OQ-13).
13. **Segment length.** Research suggests about 3 min vs the decided 8 min. 8 min is kept; handlers, selectors and fallback exist to make it work.
14. **Voting requires 4+ players** vs local host max 4 seats: voting only ever happens at exactly 4 (OQ-11). Similarly, the pity vote at 4 seats has last place voting yes, and a 2–2 tie fails.
15. **Tick rate.** Never stated; 60 Hz appears only in the superseded straw man. Assumed 20 Hz (OQ-1).
16. **Program install timing** ("~10–30 s by size") and **Firewall Vault (operator fuel)** capability belong to custom operators and are out of v1 scope. Reserved only.
17. **"Destroyed beacon → structures become unclaimed"** vs **"unfunded beacon → dormant, structures stay"**. These are consistent but need distinct statuses; the schema has `DESTROYED` and `DORMANT` separately.
18. **Brief path mismatch.** The decisions-log path given in the brief did not exist; the real file is in the hyphen-joined directory. `switchyard-spec.html` is stale and was ignored.

---

## 9. Implementation order (solo dev + AI agents)

1. **Proto and codegen** (1 week): `gp.v1`, `gp.api.v1`, custom options, descriptor → JSON Schema generator, and round-trip tests.
2. **Verifier S0–S3 + diagnostics catalogue + `render_plan`** (2 weeks), with golden tests.
3. **Plan interpreter in the sim + feed events** (2 weeks), with the termination property tests (proptest over random plans).
4. **Estimator S4 + economy projection** (1 week), reusing the Quartermaster and pathfinder.
5. **Gateway methods + MCP adapter (rmcp) + llms.txt generation** (1.5 weeks). Exit criterion: spike G6, where an agent plans a turn from llms.txt alone.
6. **Planner Easy and Normal + Safe Plan** (1.5 weeks).
7. **Godot editor**: map route, clock, rule list, templates, inline diagnostics (4 weeks). Then Hard planner and polish.

---

## Sources

- FF12 gambits: https://finalfantasy.fandom.com/wiki/Gambits
- Door Kickers 2 go codes: https://steamcommunity.com/app/1239080/discussions/0/594015574338601221/
- Gladiabots bot programming: https://wiki.gladiabots.com/index.php?title=BotProgramming_Basics
- Frozen Synapse Prime design: https://www.gamedeveloper.com/business/frozen-synapse-prime---our-recreation-and-some-of-the-challenges
- Into the Breach postmortem (GDC 2019): https://ubm-twvideo01.s3.amazonaws.com/o1/vault/gdc2019/presentations/Into%20the%20Breach%20Postmortem%20Final.pdf
- Factorio blueprint string format: https://wiki.factorio.com/Blueprint_string_format
- Desynced behaviour programming: https://wiki.desyncedgame.com/Behavior_Programming
- Opus Magnum histograms discussion: https://steamcommunity.com/app/558990/discussions/0/2381701715715906520/
- MCP tools spec (structured content, isError, pagination): https://modelcontextprotocol.io/specification/2025-06-18/server/tools
- MCP tool annotations: https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/
- MCP Streamable HTTP security (Origin validation, localhost binding): https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http
- llms.txt: https://llmstxt.org/
- rustc JSON diagnostics: https://doc.rust-lang.org/rustc/json.html
- Protobuf proto3 guide: https://protobuf.dev/programming-guides/proto3/ ; unknown fields: https://kmcd.dev/posts/protobuf-unknown-fields/
- Buf breaking rules (WIRE_JSON): https://buf.build/docs/breaking/rules/
- Buf protoschema plugins (JSON Schema): https://github.com/bufbuild/protoschema-plugins
- Anthropic, Writing effective tools for agents: https://www.anthropic.com/engineering/writing-tools-for-agents
- CivBench (agents under-monitor state): https://arxiv.org/abs/2609.02459
- Godot 4.5 accessibility (AccessKit screen readers): https://godotengine.org/releases/4.5/
- Godot Tree drag-and-drop proposal: https://github.com/godotengine/godot-proposals/issues/3201

