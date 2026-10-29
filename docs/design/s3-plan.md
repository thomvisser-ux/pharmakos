<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# S3, authoring — plan for spec section 17, the wk-34.5 row

**Not yet adopted.** This is a draft, written on 2026-10-28 under decisions-log §2.7 item 153 (5)
from the owner's four kickoff answers (item 153 (1) to (4)) and the register's S3 rows. As S2's plan
was under item 144 (8), it is reviewed twice by adversarial agents, merged by the main session
under the delegation item 152 (2) renewed for S3, and adopted on the owner's bulk answer: every
decision of sections 6 and 7 stands as its **(Recommended)** option unless the owner overrules it.
**[play]** marks a play-shaping decision, put first; **[risk]** marks a risk the owner accepts or
refuses. The reviews are AI reviews, not independent review.

**S2 is not closed.** Its demo (#156, `docs/demo/s2-run-sheet.md`) is ready and its thirteen
rulings are made (item 152 (1)), but the owner has not yet reviewed the demo itself (AGENTS.md §10
item 8). Item 153 says S3's first lanes start once that review has happened, unless the owner says
otherwise. Nothing in this plan assumes the review moves a number; if it does, the lane that owns
the number takes the ruling in its brief.

Precedence throughout: `docs/design/decisions-log.md` §2.7 > `docs/spec/pharmakos-spec-v0.6.html` >
`docs/design/co-design-gameplan-api.md` (`docs/design/README.md`); `docs/design/targeting.md` amends
the spec where it says so. The co-design doc predates v1 and its MCP tools, flags, branch, repeat
and WASM planner are not in v1. Where the spec is silent nothing is invented: it is an owner
decision with a recommendation and every option's downside (AGENTS.md §12). Engineering calls are
made here, in section 2.3, each "decided; the owner can veto".

**Baseline.** `main` at `3764369`, where item 153 (#158) has merged. S2
left more of S3's row built than its wording suggests:

| Row item | What exists at `3764369` | What is missing |
|---|---|---|
| Rule list | `godot/scripts/rule_list.gd` draws read-only rows from `render_plan`'s prose | chips, drag reordering, the family → predicate → parameter pickers, locks with their reason, the conflict connector (S3-17, S3-01, U-07) |
| Size meter | the verifier counts `size_units` (`crates/verifier/src/size.rs`); `VerifyReport` carries `size_units` and `size_budget` (`gateway.proto` ~1135) | any client that reads them; the budget, 128, is the rules row read at `crates/verifier/src/limits.rs` ~175–184, provisional |
| Probation | the lobby hosts a Probation-shaped match (`godot/scripts/host_link.gd` ~60–74: golden seed, two seats, Easy, three rounds) | a named preset, untimed Lulls by preset, the memos, the guided first pick |
| Resonance Spire | `StructureKind::ResonanceSpire`, its rules row (`structures.resonance_spire` 120 / 500 / 3), its kW draw, the verifier's blueprint list, the operator's capability weight | licences of any kind, `queue_structure` (refused `NotAtThisStage("S4")`), the larger sphere, one per beacon, the recap's line (`crates/operator/src/candidates.rs` ~101 says no licence is on any wire until S4) |
| Save and Load | built in T19 (`crates/client-gdext/src/editor.rs`, `godot/scripts/editor.gd`): JSONC with comments kept | S3-03 (`gamectl verify`'s reference view) and the S1 demo's editor findings |
| P6 | — | the timing session and its run sheet |
| G2 + P3 | HPA\* in `crates/sim/src/pathing*`; `crates/sim/tests/pathing_bench.rs` counts expansions; `crates/sim/tests/pathing.rs` holds the estimate within ±15 % p90 against its own walk model | a wall-clock measurement (walled, so in `crates/bench`), the estimate checked against the sim's own walkers, S3-10's rows, `climb_surcharge`'s evidence |
| P1 FULL | `estimate.rs` and `lint.rs` run on submit; `crates/bench` measures QUICK and the decision tick's cost per size unit | FULL's wall-clock p99 at the size budget, the budget derived from both, S1-49's bench seed |

---

## 1. Goal and end state

Spec section 17, the wk-34.5 row, verbatim: "Rule list, size meter, Probation preset with 3–4 memos
and the guided first pick · Resonance Spire with minimal licence plumbing · Save and Load of JSONC
files · P6 editor gate (template ≤ 2 min, scratch ≤ 5 min) · depth task: HPA\* pathing in the sim
and the travel estimator (G2+P3, ±15%) · exit: verifier FULL and the plan-size budget (P1 FULL)."

§2.7 has since added to it: the fun gate judged by the owner alone, with the build measuring the
playtest bundle's four metrics (item 153 (1)); the Probation preset, its memos and the guided first
pick in S3, the settings screen staying in S6 (item 153 (2), moving item 127 (10)'s Probation half
and register row S1-40's); one real licence through a general mechanism, the Spire its only entry
(item 153 (3)); the owner standing in for P6's tester (item 153 (4)); seat tokens receiving no
match seed from S3 (item 127 (13)); and the carries of items 151 (4) and 152 (1) and (3), placed in
section 2.

### 1.1 The demo the owner reviews

One unsigned Windows zip (the same build on Linux), started as `Pharmakos.exe -- --rounds 4` (item
143 (5)): the licence beat needs a fourth round (decision 13). The lobby offers one match, now
named **Probation**: the owner against Easy on the preset's map seed (decision 13), the rules' 3 / 5
/ 8 ladder with round 4 taking its last entry, untimed Lulls, and a **Playtest mode** checkbox, off
by default, which the owner ticks. As at S1 and S2 the main session drives the zip by computer use
while the owner watches, and presses Ready to end a Lull.

1. **Round 1's Lull, the guided pick.** No countdown shows: the Lull is untimed. The wizard opens
   by itself on the guided pick (decision 5): a template marked as the place to start, with the
   reason the gateway sent, the other templates still listed. The owner takes it. The size meter
   under the editor reads the playbook's units against the budget (for example 14 of 128, the
   numbers being whatever `tune3` sets).
2. **The rule list.** Each rule is a sentence with chips. The owner clicks the repair threshold's
   chip and types a new number; the gateway's verdict comes back on the row. A rule the run sheet
   added below the flee rule can never fire, and the W0702 connector joins the two; the owner drags
   it above, and the connector goes away. The owner adds a condition through family → predicate →
   parameters (own beacon → has a structure → Generator). One entry, *queue a Resonance Spire*, is
   greyed with a lock and its reason: "needs the Resonance licence: run Generators on two vents at
   a settlement". The owner adds a `toward` placement at the second vent the run sheet names.
3. **Save and Load.** The owner saves the playbook as a JSONC file, closes it, loads it back, and
   the comment typed in step 2's file is still there. A file holding a `set_flag` is refused on
   Load with its code and pointer.
4. **Round 1's Push, the first memo.** A Generator is built ahead of its supply and a beacon sheds;
   the event list shows the Ledger's power-shortfall memo in its deadpan voice (decision 4). The
   commander places the `toward` beacon, a stepping stone toward the second vent.
5. **Round 2, the licence.** The owner carries the playbook with one more `toward` step, which
   places a beacon covering the second vent, and a Generator target on it. At round 2's settlement
   the seat runs Generators on two vents: the recap shows "Licence awarded: Resonance Spire", and
   the first-licence memo explains what it now allows.
6. **Round 3's Lull.** The lock is gone. To the route's visit at the core the owner adds a
   `queue_structure` row for the Spire, tries a second at the same beacon, sees the verifier refuse
   it (one per beacon), and removes it. Seals.
7. **Round 3's Push.** The commander walks to the core and stays for the 5 s queue; the `$` leaves
   the treasury at once and the kW draw starts. The core's fabricator builds the Spire whatever the
   mandate. On completion the core's sphere ring is drawn larger (decision 6). If the owner rules
   Easy fights (decision 3), Easy's raid arrives once its sightings are stale or missing and its
   Survey post has found the owner's beacons, and the commander's first death brings that memo.
8. **Round 4 and the end.** In round 4's Lull the frozen snapshot holds the larger sphere, so the
   verifier accepts a Build target beyond the old ring, and in the Push a drone builds it. The
   match ends at round 4's limit. The playtest survey asks its three questions (decision 11); the
   owner answers, presses **Export playtest bundle**, and picks a file. The main session opens the
   zip beside the game: the survey, the metrics (Push time focused and unfocused on unskipped
   Pushes, Lull time used, skips and speed-ups, camera use, verifier errors) and the client's own
   diagnostics log (E10), and no playbook, draft, notebook or cache file.
9. **The staged beat, said to be staged.** The Spire's destruction and the sphere's return to its
   base radius is shown from `lic`'s headless scenario and its event log, since no scripted match
   is sure to destroy it. If decision 13 takes its second option, the licence beat (steps 4 to 8's
   stepping stones, licence, queue and growth) is shown the same way, from `lic`'s
   `resonance-spire` scenario, and the live demo plays the golden seed's three rounds.

**Why the fourth round (a finding, checked for this fix pass).** On every three-seat line of
`tests/golden/mapgen/expected.features.txt`, read against the cores in `expected.bore.txt`, each
seat's start vent lies 28 to 43 voxels from its core and the next vent 92 to 320 voxels away. On the
golden seed's two-seat map (`tests/golden/gateway/map_summary/expected.response.json`, seat 0's core
at about (358, 24)) the start vent is 35 voxels away and the next 230 (`vent_148_117`, travel 270 s).
One beacon's reach is 48 voxels (item 95), so a second vent is never within round 1's reach on any
seed checked, and on the golden seed it is about nine stepping stones out, beyond a three-round
Probation match. `targeting.md`'s "about 80 voxels" finding understated it. Spec §8's trigger,
"Running Generators on 2 vents", is therefore reachable in Probation only with `toward` and a map
whose second vent lies a few stepping stones away; decision 13 puts the choice to the owner.

Beside the demo: the G2 + P3 certification report and the P1 FULL report, each measured on the
owner's machine under Windows and WSL2 Linux, with the size budget's derivation (section 5); the
owner's two P6 timing records (section 5.3); the nightly report and the balance check re-run on
S3's rules; and the fun-gate readiness list of section 1.4, checked.

### 1.2 The machine-checkable half — AGENTS.md §10, item by item

| §10 item | What closes it at S3 | Task |
|---|---|---|
| 1. `cargo xtask ci` green on the matrix, the OSes agreeing | every lane green on the three-OS matrix; the cross-OS guard over every chain, including S3's new scenarios | all; `demo3` |
| 2. Hashes committed, every movement explained | the re-bless ledger of section 4.2; each chain-moving PR names the rule and the first tick that moved | `lic`, `pred`, `opr3`, `tune3` |
| 3. Golden playbooks round-trip, verify and render identically | the new constructs' canonical, verify and render goldens under `tests/golden/plan-core/` and `tests/golden/verifier/`; render's chip spans in their own golden; the library's templates re-blessed with reasons | `con6`, `gwr`, `vfy5`, `vfy6`, `opr3` |
| 4. Headless scenarios assert on events **and** hashes | new scenarios under `scenarios/s3/`: the Spire's licence, queue, growth and loss; `toward`'s stepping stone; an enemy-knowledge rule firing; a Probation match whose memos fire (their gateway half asserted in `crates/gateway/tests`) | `lic`, `pred`, `gwp` |
| 5. The nightly adversarial scenarios | the three arms on the fixed seed set, each band re-read on S3's rules, with the Rusher's launch now checked (item 151 (4)) | `nite3`, `tune3`, `demo3` |
| 6. Depth task and gate, or the written fallback | G2 + P3 certified, and P1 FULL with the size budget, by recorded runs (section 5), or the spec's fallback taken in writing | `bench3`, `cert3`, `tune3` |
| 7. Docs in the same PR | the spec amendments and AGENTS.md sentences of section 4.4, in the main session's docs PRs | main session |
| 8. The owner reviews the demo | §1.1, from `docs/demo/s3-run-sheet.md` | `demo3` |

### 1.3 What "ends playable" means here

Every item of the row, of §2.7's additions, and of the register's S3 rows and carries as section 2
places them, less whatever section 4.5's written cut order has moved out by the stage's close.
Nothing else. The rows section 2 defers keep their markers, re-dated to the stage that takes them.

### 1.4 The fun-gate readiness the build must have

The gate is a week after S3 (spec §17, wk 35.5) and the owner judges it alone (item 153 (1)), so
S3's job is a build that can be handed to 3 to 5 testers and can record what they did. At S3's
close the main session checks, and the run sheet records:

1. **The gate's match is one click.** The lobby's Probation preset is the spec's shape: two seats,
   the tester against Easy, three rounds at 3 / 5 / 8 min, untimed Lulls, on the map decision 13
   chooses.
2. **A first-time tester can file a playbook.** The guided pick opens round 1's Lull; Ready with
   nothing sealed files the safe playbook and says so; every refusal is a sentence. Spec §16: "A
   tester who cannot file a playbook is a wizard bug plus the guided first pick, never a fun
   result."
3. **The four metrics are recorded and exported.** Playtest mode collects "play another round?",
   Push time focused against unfocused on unskipped Pushes, Lull time used, and the skip rate, and
   the bundle carries them with the survey's other two answers and the client's diagnostics log,
   never a playbook, draft, notebook or the private replay (spec §15, AGENTS.md §7). No network.
4. **The zip opens on a clean machine.** `clean launch (windows-latest)` and `clean launch
   (ubuntu-24.04)` green on S3's last commit; the two-line note on unsigned builds in the zip.
5. **No numeric threshold is written anywhere.** The verdict is the owner's (item 153 (1)); the
   build and the run sheet record, they do not judge.
6. **The tester's sheet exists.** `docs/demo/fun-gate-sheet.md`: how to start the zip, turn on
   Playtest mode, and send the bundle by hand. It holds no tester's name or address.

---

## 2. What §2.7 settled for S3, and where each ruling lands

| Ruling | What it says | Task |
|---|---|---|
| 153 (1) | The fun gate is the owner's judgement, with no numeric threshold; the build measures and records the playtest bundle's four metrics | `prob` (playtest mode and the bundle); `demo3` (the readiness list, the tester's sheet) |
| 153 (2) | The Probation preset, its 3–4 memos (triggers and first copy) and the guided first pick in S3; the settings screen in S6 | `gwp` (the preset on the config line, untimed Lulls, the memos, the guided pick's mark and reason), `gwr` (the first-licence memo); `prob` (the lobby, the memos drawn, the guided pick); decisions 4 and 5 |
| 153 (3) | One real licence by a general mechanism (the settlement check, a per-seat licence set in hashed, snapshotted state, the recap's line, queue at any beacon), the Spire its only entry | `con5` (rows, wire), `lic` (the sim), `vfy5` (the checks), `gwr` (recap, reads), `opr3` (Easy), `prob` (drawn); decision 6 |
| 153 (4) | The owner stands in as P6's tester, mid-S3 | section 5.3; `p6` run sheet in the main session's docs PR |
| 152 (1) 1 | Rules revision 6 kept; every S2 tuning row moves to S3, re-read once Easy fights | `con5` re-dates `rules.proto`'s markers; `tune3` re-reads; decision 3 says whether Easy fights |
| 152 (1) 2 | The non-rules markers due at S2's close kept; S2-08 closed; the rest to S3 | `redate3` and each file's S3 owner re-date; `tune3` re-reads |
| 152 (1) 3 | The Turtle's walls (S2-17) to S5 | `nite3` re-dates `turtle.playbook.jsonc`'s marker (a JSONC file, so not the sweep's) |
| 152 (1) 4 | `atk`'s `e_NN` pin across a resume gets a contract fix in S3 | E1: the handle table in the save; `gwp` |
| 152 (1) 6 | Survey's re-scanning is decided at S3 | decision 7; `pred` |
| 152 (1) 10 | The G3′ gate load's Attack seam goes to S3's first `crates/sim` lane; `g3 budget` stays able to fail | `sim3a` (the seam), `bench3` (the load), `cert3` (the re-run) |
| 152 (1) 11 | S2-12's delta mode and S1-34's recompute key move to S3 | E15 and decision 14: measured by `cert3`; built in S3 or at S7 as decision 14 rules; until then the markers read S3's demo (`con5`, `redate3`) |
| 152 (3) | The comment-only re-dating sweep; `rules.proto`'s markers in S3's first `proto/**` PR; a guard on the Rusher's launch | `redate3`; `con5`; `nite3` |
| 151 (2) (a) | "Nearest" is straight-line from the staging point; `BeaconRef.nearest`'s comment fixed in S3's next `proto/**` PR | `con5` |
| 151 (2) (c) (1) | The balance check re-read once Easy attacks | `tune3`, under decision 3 |
| 151 (2) (c) (3) | A `combat.charge_damage_hp` row in S3's next `proto/**` PR | `con5` (row), `lic` (the sim reads it), `tune3` (its value) |
| 151 (2) (c) (4) | A verifier note that a breach needs a `FREE_FIRE` mortar | `vfy5` |
| 151 (4) | The hunts-never-launched count; the stale `AVOID_KNOWN_THREATS` marker | `nite3`; `sim3a` |
| 127 (13) | From S3 seat tokens receive no seed; the docs say a local agent can still read `match.json` | `gwp` (the seed withheld from seat-scoped answers); the main session's docs line |
| 139 (2) D, E, F, H, I, L, M, N; 143 (3), (4) | The S1 demo's editor findings, the replay's three, and "Rename/explain Ready; S3", with its two wordings | `ed3a` (client findings, Ready's label and "your orders are final for this Lull", `godot/scripts/strings.gd` ~133 and ~174), `gwp` (strings E, F, 143 (3)'s first, and `safe_playbook_filed`'s "The Lull ran out", `crates/gateway/src/strings.rs` ~460, which shows after Ready too), `opr3` (L) |
| 139 (2) C; 139 (5) | Alt-click on a vent showed the plain menu in the S1 demo; "No gate; failure → S3". No later record says the owner's hand check was made | `ed3b`: a client test that Alt-click on a vent composes the description (`covering {vent {NEAREST, UNCOVERED}}`), and the owner's hand check at P6a; a failure is fixed in `ed3b` |
| 139 (5) | P1's count carried to S3: the decision tick's cost in counted evaluation units at the size budget, added to `pharmakos-bench`'s notice | `bench3`; section 5.2 item 4 |
| 128 decision 3 (S1's plan) | `toward`, a site arm for stepping-stone expansion, deferred to S3 with the catalogue | decision 2; `con6`, `pred`, `vfy6`, `opr3` (S5-04) |
| 144 (11), decisions 13 and 16 (S2's plan) | Auto-fortify and unit mix, and `report_kinds` with `get_reports`, deferred to S3 | decision 2 (recommended: S6) |

### 2.1 The register's rows, each placed

Every row of `docs/placeholders.md` dated S3, S3's exit, S3/S6 or "with the knowledge store", the
S3 rows of other sections, and the S2 rows item 152 moved. "Re-dated" means the owning lane edits
the marker's when, citing this plan; the register's text is the main session's docs PR.

| Row | What | Where it lands |
|---|---|---|
| S3-01 | Visit & change offers the priority row only | `ed3b` (the mandate and Build-target rows, with the pickers); `gwr` (X-17's per-beacon detail they read) |
| S3-02 | The placement ghost per click, not live on hover (S3/S6) | **S6**, with the full editor; re-dated by `ed3a`. Not on any P6 path: a ghost per click still answers |
| S3-03 | `gamectl verify` keeps its reference view, so E0403 on a beacon the game accepts | `nite3`, by E19 |
| S3-04 | A seat's sighted enemy beacons join the verifier's scope | `vfy5` (the scope's field), `gwr` (filled) — needed by the enemy-knowledge predicates |
| S3-05 | Real terrain fog as one mask for the view and the route estimator | **S4**, decision 2 |
| S3-06 | `MAX_ROUTE_STEPS` 256 and `MAX_HANDLERS` 64 | `tune3`, settled with the size budget (E17) |
| S3-07 | A step's tags counted, not stored; a selector's tag filter refused | **S6**, decision 2 |
| S3-08 | An absent `on_fail` means `SKIP` | decision 8; `vfy6` (its I0003 note) |
| S3-09 | `max_deaths_before_fallback` 0 means no limit | decision 8; `vfy6` |
| S3-10 | `LONG_RUN` 6 and `SMOOTH_LOOKAHEAD` 16 as rules rows | `con5` (rows at today's values), `lic` (the sim reads them), `cert3` (measured) |
| S3-11 | The corner entrance, recorded and not built | E16: stays unbuilt unless P3 misses; `cert3` reports |
| S3-12 | `TARGETS_PER_BEACON` 8, settled with the size budget | `tune3` (E17) |
| S3-13 | `SIGHTINGS_PER_SEAT` 64 | `tune3` (E17), from `cert3`'s peak count |
| S3-14 | No probe area recorded as visited | **closed**: `surv` #145 wrote the explored table, and the grep at `3764369` finds no marker; the register's text closes it |
| S3-15 | Condition depth and node limits need rows if they become tuning | **closed** by `vfy6`: they are the spec's own limits (§10: depth ≤ 4, ≤ 24 nodes), technical, not tuning, so no row |
| S3-16 | The segment clock and the "fits" pill (S3/S6) | **S6** (spec §17's S6 row names the segment clock); re-dated by `ed3a` |
| S3-17 | Chip editing and drag reordering | `ed3b` |
| S3-18 | Travel times as raw game milliseconds | `gwr` (the gateway answers a rendered figure, items 57 and 61), `ed3b` (drawn) |
| S3-19 | The verifier's travel is a lower bound; pathfinder-quality legs with the segment clock | **S6**, with S3-16; re-dated by `vfy5` |
| S3-20 | The `on` sphere test's residual; S3's fog bases it on knowledge | **S4**, with S3-05 |
| S3-21 | `bmi_next_dollars` answered in a Push | **S4**, with S3-05 |
| S3-22 | A paid structure under a later protected area stays unfinished | decision 8 |
| S3-23 | A fog-filtered terrain summary in `get_map_summary` | **S4**, with S3-05; re-dated by `con5` |
| S1-46 | The forecast's what-ifs, projected income and projection | **S6**, with the segment clock; re-dated by `con5` (proto) and `gwr` (code) |
| X-15 | `GetBriefingResponse`'s structured payload | **S6**, with the notes box; re-dated by `con5` |
| X-17 | `GetBeaconResponse`'s per-beacon detail | `con6` (fields), `gwr` (filled): S3-01's pickers read it |
| S5-04 | One expansion per goal; stepping stones with `toward` | `opr3` (Easy's stepping stones with `toward`, never cut under decision 13's recommendation) |
| S1-40 | The lobby's match; the settings screen and Probation | `prob` (the preset); the settings-screen half stays **S6** and its marker says so |
| S1-49 | `BENCH_SEED`, P1's map | `cert3`: kept, or a second seed added, as P1 FULL's report shows (E12) |
| S1-52 | `CORE_DIG_MAX_DEPTH` 4, re-run once Easy reaches past one beacon | `opr3` re-dates it to S3's demo, where `tune3` reads it in the balance re-run; to S5, with Normal and Hard, if `toward` leaves S3 (decision 13's alternatives) |
| S2-03 | `AVOID_KNOWN_THREATS`'s stale marker | `sim3a` deletes it |
| S2-12 | `get_view`'s delta mode | decision 14 (recommended: measured by `cert3`, built at **S7**); until it is ruled, `con5` dates the marker "owner, at S3's demo" |
| S1-34 | The view's recompute key | decision 14, as S2-12; until ruled, `redate3` dates the marker (`watch.rs` ~150) "owner, at S3's demo" |
| S2-19 | `TOP_UP_TICKS`, five seconds | `tune3` (with S2's other rows) |
| M-12 | An area is an axis-aligned box | decision 8: kept; `con6` rewords the marker as ruled |
| M-13 | A patrol route is a list of locations | decision 8: kept; `con6` rewords |
| M-14 | The mandate messages in `gp/v1/mandate.proto` | E20: not moved; `con5` closes both copies' markers |
| U-07 | The full predicate catalogue is S3's, and no stub says so | `con6` writes the stub naming what S3 fills and what S6 does (decision 2) |
| U-08 | Route figures priced over unseen edits until the fog mask | **S4**, with S3-05 |
| S5-14 | Spec §14's Survey post, Defend guard, chatter and target spread not built at Easy (`crates/operator/src/lib.rs` module doc; item 111, A6); the Defend guard was built by `oprd` #144, so the row is partly discharged | the Survey post: `opr3`, under decision 3, on spec §14's trigger; chatter and target spread stay **S5**; `opr3` rewords the module doc's marker to what is left |

**The delegated tuning rows** (the register's S3 list, 8 rows): `match.decision_tick_ms`,
`structures.resonance_spire`, `verifier.handler_cooldown_min_ms`, `max_fires_max` and
`notebook_max_chars`, `verifier.reach_memory_ms`, `verifier.size_budget_units` (S3's exit) and
`locomotion.climb_surcharge` all go to `tune3`, which reports each against the measurement that bears
on it (the size budget against P1, `climb_surcharge` against `cert3`'s walked-against-estimated
legs on slopes, `reach_memory_ms` against Survey's refresh and the Attack pin) and re-dates every
marker it keeps to S3's demo. **S2's rows** moved by item 152 (1) 1, and the economy, power and map
rows the register gives S3, are re-read by `tune3` with the balance check once Easy fights, and
re-dated in `rules.proto` by `con5`. **The non-rules markers** item 152 (1) 2 kept (the empty
Attack and Defend arms' readings, `BREACH_COLUMN_COST`, Survey's cell and refresh, the fielding
totals, the site top-up, the commander's speed, the audit, the balance check's seeds and flags, and
the repair threshold of 50 in three JSONC files) are re-dated to S3's demo by `redate3` where it
may reach them, and by `sim3a`, `nite3` and `opr3` in their own files.

### 2.2 Carries not in the register, each placed

| Carry | Source | Where it lands |
|---|---|---|
| The comment-only re-dating sweep | item 152 (3); AGENTS.md §6 | `redate3`, granted there |
| `rules.proto`'s markers | item 152 (3) | `con5` |
| A guard on the Rusher's launch | items 151 (4), 152 (3) | `nite3` (E14) |
| The nightly result's count of hunts that never launched | items 150 (2), 151 (4) | `nite3` (E14) |
| The `e_NN` pin across a resume | item 152 (1) 4 | `gwp` (E1) |
| The G3′ gate load's Attack seam | item 152 (1) 10 | `sim3a`, `bench3`, `cert3` (E13) |
| `BeaconRef.nearest`'s proto comment | item 151 (2) (a) | `con5` |
| `combat.charge_damage_hp` | item 151 (2) (c) (3) | `con5`, `lic`, `tune3` |
| A verifier note for a `RETURN_FIRE` breach | item 151 (2) (c) (4) | `vfy5` |
| The stale S2-03 marker | item 151 (4) | `sim3a` |
| `nightly-scenarios.yml`'s `not-yet-enabled` summary text that still calls the arms placeholders | item 151 (4) | `cert3`, the stage's one `.github/**` lane |
| Item 148 (4)'s proto comments not yet landed: `MineSettings.flee_on_threat`, `BuildSettings.rebuild_destroyed` and `repair_threshold_pct` `optional`; `BuildTarget.wall_box`'s comment; `ViewEntity.id`'s comment; `playbook.proto`'s header on an unset `roe`'s default by writ | S2's plan §2.2, last rows | `con5`, each checked against `main` first (some may have landed with `con4` #120) |
| `situation.rs` ~619, a resumed Lull forgetting a death, accepted as a gap | item 152 (1) 7 | `opr3` re-dates it to S5, with Normal and Hard |
| `BREACH_COLUMN_COST` and the breach search's budget (`attack.rs` ~133, ~140) | item 152 (1) 2; the register | `cert3` measures the breach search at the gate load; `tune3` re-dates |
| The avoid-area reserve (`world.rs` ~301, "with P1's size budget") | the code | `tune3` (E17) |
| `toward`, `RICHEST`, grade filters, `in_reach` and enemy-known filters, fog-based liveness | S1's plan §8 | decision 2 |
| `get_reports`, `list_known_enemies`, `query_area` (each `INTERNAL` today) | S2's plan §8 | decision 2 (S6 and S4) |
| The no-silent-fallback sweep | the operator's global rules; AGENTS.md §6 | each lane, in the crates it owns (standing rule) |

### 2.3 Engineering calls, decided here (the owner can veto each)

Hygiene, types, layout, wire shape and test gates, under the operator's global rules and AGENTS.md
§4. Each is the main session's call; where one carries a reading of play it names the decision that
puts that reading to the owner.

- **E1. The `e_NN` handles live in the save, not in the sim.** Item 152 (1) 4 offers two fixes:
  first-sighting order in the sim, or the handle table in the save. **The handle table in the save
  is taken.** A handle is the gateway's per-viewer name (`name_sightings` in
  `crates/gateway/src/surface/knowledge.rs`, "the one place a seat's `e_NN` handles are minted");
  putting its order in the sim makes a presentation detail hashed state, moves every chain, and
  still renames a beacon the forget rule dropped and a later sighting brought back, because the
  sim's table no longer holds it. The save holding each seat's `(beacon id → handle)` table
  reproduces the names exactly across any number of resumes. `SAVE_FORMAT_VERSION` 2 → 3
  (`crates/gateway/src/save.rs` ~99), a format on a player's disk (AGENTS.md §5); a version-2 save
  is refused with the typed `INVALID_ARGUMENT` the version check already gives, naming both
  versions. The regression test comes first: a match resumed at round 3's Lull names every `e_NN` as
  the unresumed match did, and an Attack pinned by handle in round 2 hits the same beacon. `gwp`, in
  the same bump as E5's memo set.
- **E2. The licence set is a sparse table, hashed as a block that is empty until a licence is
  earned.** Rows are `(seat, licence, round_awarded)`, ordered by seat then licence; the block
  encodes nothing while empty (AGENTS.md §4.8, item 136's rule), so a chain whose seats earn nothing
  does not move. The three places in one PR: the state hash, the snapshot (`SNAPSHOT_VERSION` 14 →
  15, an older snapshot refused with its typed version error), the goldens. The licence is one
  proto enum, `gp.v1.Licence` (`LICENCE_UNSPECIFIED` 0, `LICENCE_RESONANCE_SPIRE` 1), added by `con5`
  and shared by the sim, the verifier and the gateway's wire, so no layer spells a licence as a
  string; the sim's table stores its value, and S4 adds values at the end. The check runs once per
  settlement, after income, in seat order; an eliminated seat's rows are removed on the tick it is
  eliminated (spec §8: "An eliminated seat's licences are voided"). Two event kinds are added at the
  end of `EventKind`, `licence_awarded` and `licence_voided`, with new ids (AGENTS.md §5's additive
  rule). `lic`.
- **E3. A beacon's sphere radius is derived, never stored.** Radius = `beacon.sphere_radius_voxels`
  plus the Spire's bonus row when a finished Spire stands on that beacon; the structure rows are
  already hashed, so no column is added. One sim function answers it, and every reader decision 6
  says grows with the Spire calls it; the readers it says do not grow keep the base row, by name.
  A test lists both sets and fails on a reader in neither. The gateway serves the grown radius per
  beacon as an `optional uint32 sphere_radius_voxels` on `BeaconSummary` (additive), so the client
  draws a ring it never computes, and the operator and the verifier read the grown radius through
  the gateway's view, never from the rules row. The verifier's `Scope` takes it **through a
  separate setter** (a per-beacon radius table keyed by beacon id, defaulting to nothing, with a
  typed error for a beacon the scope does not hold), so `KnownBeacon`'s literal constructions in
  `crates/gateway/src/surface.rs`, `crates/gateway/src/surface/knowledge.rs`,
  `crates/gamectl/src/seat.rs` and `crates/bench/src/quick.rs` do not change; it is a `report_hash`
  input.
- **E4. `queue_structure` is lifted for the Spire alone.** Today the verifier refuses every
  `queue_structure` row with E0009 (`crates/verifier/src/refusals.rs` ~85, `strings.rs` ~106) and
  the sim with `NotAtThisStage("S4")`. From S3 both keep that refusal for every blueprint but
  `resonance_spire`; for the Spire the verifier emits E0507 ("needs a licence this seat does not
  hold", `strings.rs` ~466) when the frozen snapshot's seat does not hold the licence. E0506 ("not a
  blueprint the catalogue holds", `catalogue.rs` ~712, `NoneYet` today) stays unemitted: an unknown
  id is still E0009's until S4's catalogue. One Spire per beacon is refused twice: by the verifier
  at plan time (a new E-code for a second queue at one beacon in a playbook, or for a beacon where
  one stands in the frozen snapshot) and by the sim at run time (a new `StepFailure`,
  `spire_present`, at the end of the enum), since the plan-time answer can go stale. The queue reads
  `interface.rs`'s existing `queue_structure_ms` (5 s); the `$` leaves through the Quartermaster at
  the queue, the kW draw starts with it (spec §8).
- **E5. The memos are the gateway's, derived from the event bus.** The sim gains nothing. For the
  human seat of a Probation match the gateway watches the feed it already filters, issues each memo
  at most once a match on its trigger's first event, puts it on the feed as a new kind,
  `ledger_memo`, with its id and text, and lists the match's memos on the recap. The issued set is
  saved (E1's version bump), so a resume never repeats one. The copy lives in the gateway's string
  table, English, one place (AGENTS.md §12). Decision 4 picks the triggers and the voice. `gwp`
  issues the first three; the first-licence memo is `gwr`'s, since it keys on `lic`'s
  `licence_awarded`, which merges after `gwp`.
- **E6. The preset travels on the config line.** `serve::Config` gains a `preset` field (`probation`
  or `custom`; the lobby only ever writes `probation` until S6's settings screen). The line is a
  contract format the lobby remembers in `user://last_match.txt` (AGENTS.md §5); the reader refuses a
  line without the field with a typed error naming it, and the lobby, on that refusal, offers a fresh
  match rather than guessing (no silent default). `--rounds` keeps working as item 143 (5) built it.
  The lobby's half, `godot/scripts/host_link.gd`'s `config_line` writing the field and its pins in
  `crates/client-gdext/tests/godot_project.rs`, ships **in `gwp`'s own PR** as a named place, so
  `main`'s lobby and the headless watch check never see a gateway that refuses the lobby's line;
  section 4.1 schedules `gwp` where no client lane runs. `prob` then draws the preset.
- **E7. Untimed Lulls by preset.** A Probation match's Lulls are untimed: the gateway answers
  `_status.untimed`, which the rig already reads to hide the countdown, so the client makes no
  decision about the preset. The client still reports its clock through `report_host_clock`, so
  rate limits and token expiry advance (AGENTS.md §4.5); a report that carries a Lull countdown in
  a Probation match is refused with a typed error (`INVALID_ARGUMENT`, naming the preset), never
  clamped or ignored. The rules' `match.lull_ms` rows are unchanged; the preset overrides them,
  which the config line's `preset` makes explicit.
- **E8. The rule list's wire.** Three things, all gateway answers, so the client computes nothing
  (AGENTS.md §3 rule 4):
  - `render_plan` returns, beside each rule's sentence, its **chips**: for each editable value, its
    JSON Pointer into the playbook, the span of the sentence it occupies, its kind (integer with
    bounds, enum with its values, reference) and, for an integer, the rules table's bounds. The
    prose itself is unchanged, so `render_plan`'s prose goldens do not move; the chips get their own
    golden. `plan-core`'s `render.rs` builds them, `gwr` owns both crates.
  - A **vocabulary** read, `get_vocabulary` (scope `docs`, read-only, agent-shaped, AGENTS.md §2):
    the condition families, each predicate with its parameters, and each mandate, step and blueprint
    a playbook may name, every entry with a `locked` reason when this seat cannot use it yet (a
    licence it does not hold; "from S6" for a reserved block). The pickers draw it as sent.
  - **Reordering is a JSON Patch `move`** the client composes from a drag (rule 4 already allows a
    patch composed from a click); `patch_plan` answers the text. The **connector** is drawn between
    the two rules a W0702 row names; the verifier, not the client, decides two rules conflict.
- **E9. The size meter draws what the verifier counted.** `size_units` over `size_budget`, from the
  last verify answer; the bar's fill is a display ratio (rule 4's list gains it, as `vista.gd`'s
  HP bar did). Over budget, the meter shows the verifier's refusal sentence, not its own.
- **E10. Playtest mode lives in the client, local and opt-in.** `crates/client-gdext` (walled, so
  it may read the host clock) counts the metrics: Push time with the window focused and unfocused,
  per unskipped Push (Godot's focus notifications), Lull time used, skips and speed-ups, camera
  moves, verifier error rows shown. `godot/` draws the end-of-match survey and **Export playtest
  bundle**, which writes one zip through Godot's `ZIPPacker` to a path the tester picks:
  `survey.json`, `metrics.json`, `diagnostics.log`, and a `manifest.json` that lists them and says
  the recording is absent until S7's recordings exist. **The raw `godot.log` is never shipped:** it
  can hold absolute paths, and with them the tester's user name. `diagnostics.log` is a log the
  client writes itself while Playtest mode is on, of its own typed lines (event, refusal code,
  phase, host-clock milliseconds), with no absolute path, user name, machine name, playbook text or
  notebook text in it; a test pins its lines' shape and fails on a line that holds a path
  separator after a drive letter or a home folder. A test holds the bundle's entry list to those
  names and asserts no path from the private match cache, the editor's drafts or the notebook can
  be added. **The no-network check is extended** from `crates/gamectl/tests/confinement.rs`'s
  no-HTTP-client rule to `crates/client-gdext` (no HTTP or socket crate beyond the gateway's
  localhost link) and to `godot/` (no `HTTPRequest`, `HTTPClient`, `StreamPeerTCP` to a non-local
  host, or `PacketPeerUDP` in any script or scene). Off by default; nothing is collected while it
  is off. AGENTS.md §3 rule 4 gains the
  metrics' counters as pieces that count without deciding.
- **E11. P3's accuracy is a deterministic test; its latency is the bench's.** Accuracy needs no
  clock: `crates/sim/tests/pathing.rs` gains a case that walks real commanders through the tick
  (not the test's own walk model) over the estimated routes on three seeds' static terrain, and
  asserts the p90 of |estimate − walked| ≤ 15 % and the estimate never optimistic, on every OS in
  CI. The wall-clock halves (repath p99 ≤ 5 ms, estimate ≤ 1 ms) are measured only in the walled
  `crates/bench` (AGENTS.md §4.5, `pathing_bench.rs`'s own module doc says so).
- **E12. Two alarms, outside the step table, as G3′'s is.** `cargo xtask p3-budget` and `cargo xtask
  p1-budget` run the benches once on a hosted runner and fail only at the gate's numbers doubled
  (repath p99 over 10 ms, estimate p99 over 2 ms; QUICK p99 over 10 ms, FULL p99 over 100 ms at the
  size budget) or on a missing verdict, in one job, `authoring budgets (ubuntu-24.04)`, which is
  not required and can go red; `scripts/merge-train.sh` stops on it as on `g3 budget` (a harness
  edit, the main session's). The gate is the owner's-machine certification (section 5), never the
  job. `BENCH_SEED` (S1-49) stays the committed scenarios' seed unless P1 FULL's report shows a
  second seed's p99 more than 20 % above it, when `cert3` adds that seed and reports both.
- **E13. The gate load's Attack seam is a layout field, added additively.** The layout gains its
  mandate settings without changing `runner::LayoutBeacon`'s existing literal: a layout-level list
  of `(beacon index, mandate settings)` rows beside the beacons, or a constructor that takes them,
  so `crates/bench`, `crates/gamectl/tests/balance.rs` and `crates/gateway/tests/view.rs`, which
  build `LayoutBeacon` literals, compile unchanged. The bench's layout then places Attack beacons
  with their launch settings and the G3′ segment launches raids (`g3r` #147's question); `bench3`
  builds that load, `cert3` re-runs it. The seam is used only by `Host::seed_layout`; no seat
  reaches it. `cert3` re-runs G3′ once on it and records the figure
  beside S2's certified 12.43 ms; the gate itself is not re-opened unless the figure passes 25 ms.
- **E14. The Rusher's guard and the hunts count are additive fields of the nightly result.** Per
  seed, `launches` (each seat's Attack launches, by round) and `hunts_unlaunched` (hunts filed and
  never launched); the format string stays `pharmakos.nightly-result.v1`, as `tune2` kept it when a
  key left (item 151 (2) (c) (5)), because the change only adds keys. A per-PR test in
  `crates/gamectl/tests` plays the Rusher's own seed to round 2 and asserts it launches.
- **E15. S2-12's delta mode and S1-34's recompute key are measured in S3.** `cert3` publishes, as
  notices, the full entity list's bytes per poll at the G3′ load and the recompute's time per
  sensor move. Whether either is then built in S3 or at S7 is a priority call, since item 152 (1)
  11 moved both to S3: decision 14. Until it is ruled, their markers read "owner, at S3's demo".
- **E16. The corner entrance stays unbuilt unless P3 misses.** S3-10's two values become rows at
  their constants (no chain moves); the corner entrance (S3-11) is built only if `cert3`'s repath
  or accuracy figure misses, as the first engineering lever before the spec's fallback.
- **E17. The table rooms are settled with the size budget, not before it.** `TARGETS_PER_BEACON`,
  `SIGHTINGS_PER_SEAT`, `MAX_ROUTE_STEPS`, `MAX_HANDLERS` and the avoid-area reserve each follow
  from the budget B that `tune3` sets: a playbook at B must fit every room (a test asserts it for
  the worst-shaped playbook at B: all targets, all handlers, all areas), and a room larger than B
  needs is kept only where a measured peak needs it (`cert3`'s peak sightings per seat at the gate
  load). Each stays a named constant or becomes a row as the register says; none is a balance cap.
- **E18. Seed withholding (item 127 (13)) is a field left unset.** `get_map_summary`'s seed is not
  filled for a seat token from S3 (the spectator and admin tokens keep it); the field number is
  kept, its comment says so (`con5`), and a test asserts a seat's answer carries no seed. The
  generated map is still served whole (item 107 (1)); the docs say a local agent can read
  `match.json`, as the ruling requires.
- **E19. `gamectl verify` says which view it checks against.** S3-03 closes without reading a save
  outside `serve.rs` (AGENTS.md §3 rule 2's save seams): the CLI's report heads its output "checked
  against the reference view" and, on E0403, adds that in a match the gateway checks against the
  seat's own frozen snapshot, where the beacon may be legal. A golden pins the line.
- **E20. The mandate messages stay in `playbook.proto`.** M-14's `mandate.proto` move would renumber
  nothing but would move every generated path and the schema goldens for no reader; both copies of
  the marker are closed by `con5` with that reason.
- **E21. One `proto/**` PR, one `xtask` or `.github/**` PR, at a time.** `con5`, then `con6`, then
  `tune3` (its `rules.proto` marker re-dates) for `proto/**`; `bench3` (xtask), then `cert3` (xtask
  and `.github/**`) for the harness. A lane that finds it needs another such edit stops and asks to
  be sequenced.
- **E22. Every lane sweeps its own crates for silent fallbacks.** The S2 sweep's remaining counts
  (S2's plan §2.2) are each crate's next owner's, as a commit of its own in the lane's PR, under
  the global rule: a typed error, never a default on error.
- **E23. `toward`'s "nearest" is targeting's.** `toward {feature}` resolves to the legal site,
  inside the seat's spheres, nearest the named feature, where "nearest" is `targeting.md`'s metric
  (~149): the least estimated travel, the item-61 estimator's integer cost, from the origin (here
  the candidate site) to the feature's anchor column, over the terrain the seat knows; unreachable
  candidates are skipped; ties go to the lowest anchor y, then x. Straight-line distance is not
  used: item 127 (13)'s targeting design rejected it for "nearest", and item 151 (2) (a)'s
  straight-line reading is confined to an Attack target from its staging point. A `toward` that
  finds no site says why, with how many sites it weighed.
- **E24. The size budget's mechanics.** `bench3` measures QUICK, FULL and the decision tick at 64,
  96, 128, 192 and 256 units, so B is read from a curve rather than guessed. Of the sizes that pass
  section 5.2's three tests, B is the largest multiple of 16, never above 256 (the interpreter's step
  ceiling, S3-06) and never below the largest shipped template with room for the beginner's four
  rules (spec §13). Test (c) is measured, not summed: the G3′ segment is run with `bench3`'s
  generated playbooks at size B sealed for every seat, and its tick p99 must stay ≤ 25 ms, so the
  rules' cost is counted once, inside the tick it runs in. The budget the verifier and the meter
  read is the rules row, read at `crates/verifier/src/limits.rs` ~175–184; nothing restates it.
  Decision 9 keeps only the risk call on the number's direction.
- **E25. The guided pick's mark and reason come from the gateway.** In a Probation match's first
  Lull, the template list's answer marks one template as the place to start and carries the
  sentence that says why, both from the advisor's suggestion the gateway already files for a human
  seat (AGENTS.md §3 rule 3); the wizard draws the mark and the sentence as sent. No GDScript names a
  template (`crates/client-gdext/tests/wizard.rs`' `the_client_names_no_template` stays green), so
  the client decides nothing (rule 4). `con6` adds the two fields, `gwp` fills them, `prob` draws
  them.

---

## 3. The tasks

Conventions for every task, as in S2: its own worktree (`git worktree add ../pharmakos-<id> -b
<branch> main`), target directory `D:/build/<id>` with a `CACHEDIR.TAG`, `CARGO_TARGET_DIR` set on
every cargo and `git commit` command (item 151 (3)), a commit-message file of the lane's own, **one
crate one agent** (AGENTS.md §6), `main` taken in only by a signed merge (`git merge --no-ff
--no-commit FETCH_HEAD`, then `git commit -s`), never a rebase or a force-push, never
`--no-verify`, conventional commits with the co-author line above the sign-off, SPDX headers,
`cargo xtask ci` green before the PR opens, and every guessed value a `// PLACEHOLDER: <what> —
<who>, <when>` listed in the PR body. A lane opens its PR and stops; the main session merges (item
152 (2)). "Contract PR: yes" means an AGENTS.md §5 path; the PR body names the paths. Estimates
are agent-days (ad) including iteration to green, not review latency. Every lane brief carries the
rule that no personal data leaves the box.

**Standing rules carried from S2.** E21's one-at-a-time rule for `proto/**`, `xtask` and
`.github/**`. Each `→` in section 4.1 is a separate `wave-lanes.js` run; each task's Implements line
is its lane's `items`. Every task over 6 ad carries a split seam written before it starts. A lane
that adds a committed scenario owns its path constant, its `SCENARIOS` entry in
`crates/gamectl/tests/scenarios.rs` and its line in `the_committed_list_is_the_whole_set`. A lane
that changes `library/`, `rules/`, `packaging/`, `LICENSES/`, `REUSE.toml` or `godot/` is packaged
locally by the main session before it merges. A lane that changes `godot/scripts/` runs the headless
watch check by hand with the `client extension` job's command (item 143 (2)). Every lane re-dates,
in the files it owns, any S2-dated marker `redate3` had to leave out, to S3's demo with the ruling
that kept it (item 152 (1) 1 or 2).

**Named places, as in S2.** A sim lane that lifts a `NotAtThisStage` refusal is granted the
verifier's E-code and case for it and the gateway's row in `crates/gateway/tests/methods.rs`'
`mandate_and_rule_gaps()`, while no running lane owns those crates; otherwise it makes the edit as
its last commit after that lane merges. A sim lane that adds an `EventKind` or a `StepFailure` is
always granted their arms in `crates/gateway/src/strings.rs` (`event_text`) and
`crates/gateway/src/surface.rs` (`audience_of`) and the tests that pin them (S2's E17). A lane whose
change moves `report_hash` is granted, for the re-bless alone, `tests/golden/gateway/demo_*_verify/`,
`tests/golden/gateway/walkthrough/`, `tests/golden/operator/wizard/*/` and
`godot/fixtures/rows_report.json`; where a running lane owns one, the second to merge re-blesses
(S2's E33).

### `redate3` — the comment-only re-dating sweep (item 152 (3))

- **The grant, under AGENTS.md §6's comment-only sweep.** This lane may edit **comments and nothing
  else** in: `crates/gamectl/tests/balance.rs` (2 markers, ~146 and ~1331) and
  `crates/gateway/src/surface/watch.rs` (1, ~150, S1-34's recompute cost). **The change the grant
  names:** each marker whose when names S2's demo, S2's close, S2's exit or "at S2's G3′-real" keeps
  its what and its who; its when becomes "owner, at S3's demo", with the ruling cited (item 152 (1)
  2 for the balance check's two, "with `tune3`'s balance re-run"; item 152 (1) 11 for `watch.rs`'s,
  "measured by `cert3`, built as decision 14 rules"). No marker is added or removed; each keeps the
  grammar. The PR shows `cargo xtask placeholders` before and after; the main session reads by hand
  any line `git diff -U0` shows outside a whole-line comment.
- **Not covered, and why:** `rules.proto`'s markers and the generated tree go with `con5` (a `.proto`
  comment regenerates the prost tree); `gateway.proto`'s delta-mode marker with `con5`; the sim's
  markers (`attack.rs`, `tables.rs`, `survey.rs`, `world.rs`, `world/site.rs`, `rules.rs`,
  `audit.rs`, `interpreter.rs`) with `sim3a`, which launches beside this lane and owns the crate;
  `library/hold_and_build.jsonc`, `scenarios/skeleton/against-easy.playbook.jsonc`,
  `scenarios/nightly/turtle.playbook.jsonc` and `godot/fixtures/instantiate_suggested.json` are JSONC
  or JSON data, never covered (AGENTS.md §6), and go with `opr3` and `nite3`.
- **Branch** `docs/redate-s2-markers`. **Implements:** item 152 (3). **Needs:** this plan adopted.
  **Acceptance:** `cargo xtask placeholders --check` green; no golden moves. **Contract PR:** no.
  **Agent-days:** 0.5.

### `con5` — S3's first `proto/**` pull request: rows, comments and the licence wire

- **Owns:** `proto/**`, the generated prost tree (by `buf generate` only), `rules/rules.v1.json`,
  `rules/README.md`, `tests/golden/proto/`, the schema and docs goldens the regeneration moves, and,
  for the re-bless alone, `tests/golden/scenarios/nightly-result/`, which carries `rules_hash`.
  **Branch** `feat/proto-s3-rows`.
- **Builds:**
  - **Rows** (rules revision 7, each `PLACEHOLDER: tuning — owner, at S3's demo`): S3-10's
    `pathing.long_run` 6 and `pathing.smooth_lookahead` 16, at the constants' values;
    `combat.charge_damage_hp`, proposed at the mortar's splash damage (item 151 (2) (c) (3), the
    value `tune3`'s); `structures.resonance_spire_sphere_bonus_voxels`, at decision 6's number;
    `licences.resonance_vents` 2, spec §8's "Generators on 2 vents". A new row block's message is
    added at the end of `RulesTable` with a new number.
  - **The licence wire:** the enum `gp.v1.Licence` (E2); `GetRecapResponse` 6, `repeated
    gp.v1.Licence licences_awarded` (the calling seat's own between rounds, every seat's at match
    end, item 145 (7)'s fog rule); the own seat's held licences on `GetBriefingResponse` (one field
    of X-15's held range, the rest still held); `BeaconSummary`'s `optional uint32
    sphere_radius_voxels` (E3); `QueueStructureRow.blueprint_id`'s comment naming `resonance_spire`
    as the one S3 accepts.
  - **The memo kind:** the `ledger_memo` feed kind's payload (a memo id and its text), so `gwp` can
    issue memos before `con6` lands.
  - **Comments:** every `rules.proto` marker re-dated (item 152 (1) 1: the S2 rows to "owner, at
    S3's demo, with `tune3`'s balance re-run once Easy fights", the power rows with the map rows,
    `tune2`'s question 2); `BeaconRef.nearest`'s comment to item 151 (2) (a)'s reading (straight-line
    `Sq` from the staging point for an Attack target, ties to the lowest id); `GetViewResponse.
    entities`' delta-mode marker to "owner, at S3's demo" until decision 14 is ruled; X-15, S1-46
    (two copies) and S3-23's markers to S6, S6 and S4 (decision 2); M-14's two copies closed (E20);
    `GetMapSummaryResponse`'s seed field commented as unset for a seat token (E18); and section
    2.2's item 148 (4) list, each checked against `main` first, `optional` added only where a field
    has no presence today.
- **Implements:** items 151 (2) (a), (c) (3); 152 (1) 1, (3); 153 (3)'s wire; register S3-10,
  S2-12 (marker), X-15, S1-46, S3-23 (markers), M-14. **Needs:** this plan adopted; decision 6's
  number. **Acceptance:** `buf breaking` green in `WIRE_JSON` mode; `crates/proto/tests/generated.rs`
  green under `PHARMAKOS_REQUIRE_TOOLS=1`; no chain moves (nothing reads a new row yet);
  `crates/proto/tests/proto.rs` holds every new row to its README line. Each field that fills a
  held number (`GetRecapResponse` 6, the briefing's, `BeaconSummary`'s) adds the scoped
  `ignore_only` block to `proto/buf.yaml` with each discharge written out (AGENTS.md §5), and
  `con6`, if it discharges nothing, deletes it; if `con6` discharges too (X-17), it rewrites it.
- **Contract PR:** yes (`proto/**`, `proto/buf.yaml`, `rules/rules.v1.json`, golden contents).
  **Moves:** `rules_hash`, so every `report_hash` golden and the nightly result golden (granted
  re-blesses). **PLACEHOLDERs:** the five new tuning rows, listed. **Agent-days:** 3.

### `sim3a` — `crates/sim`: S2's carries and the gate load's Attack seam

- **Owns:** `crates/sim` (the first S3 sim lane). **Branch** `feat/sim-s3-carries`.
- **Builds:** E13's seam, additively (the layout-level list of mandate-settings rows, or a
  constructor; `LayoutBeacon`'s existing literal unchanged, so `crates/bench`,
  `crates/gamectl/tests/balance.rs` and `crates/gateway/tests/view.rs` compile as they are), read
  only by `Host::seed_layout`, with a test that a seat cannot reach it; S2-03's stale
  `AVOID_KNOWN_THREATS` marker deleted (`interpreter.rs` ~635); the sim's S2-dated markers re-dated
  to S3's demo with item 152 (1) 2 cited (`attack.rs` ~133, ~283; `tables.rs` ~4710; `survey.rs`
  ~132, ~139; `world.rs` ~146, ~166, ~181; `world/site.rs` ~2122; `rules.rs` ~175, ~180, ~198;
  `audit.rs` ~34); E22's sweep over whatever `base` and `ids` left in the sim; under decision 13's
  recommendation, the Probation seed: a test-only, deterministic search over seeds for a two-seat
  map whose second vent lies within three stepping stones of seat 0's core, its criterion and its
  answer recorded in the PR (the test pins the answer, so the generator cannot drift under it).
- **Implements:** items 151 (4), 152 (1) 10; decision 13. **Needs:** this plan adopted.
  **Acceptance:** no chain, no golden moves; a layout with an Attack row seeds and launches in a
  bench-shaped test; the three crates above build without an edit. **Contract PR:** no (the layout
  seam is host-side, not the tick loop). **PLACEHOLDERs:** none added. **Agent-days:** 2.

### `ed3a` — `crates/client-gdext` + `godot/`: the size meter and the editor's findings

- **Owns:** `crates/client-gdext`, `godot/`. **Branch** `feat/client-editor-fixes`. It runs first in
  slot 3 and merges before `gwp` starts, so `gwp`'s lobby edit meets no running client lane.
- **Builds:** the size meter (E9); the S1 demo's findings: D (the right panel clipped), H (a stale
  status line after Undo), I (Escape closing the map menu), M (the unticked checkbox's box drawn), N
  (a second instance no longer rotates the first's `godot.log`: each instance logs to its own
  file); item 143 (3)'s second and third (the safe-playbook line cleared at the next Lull; the Load
  dialog's stray "there"); item 143 (4)'s Ready, renamed "Ready — end the Lull" with its one-line
  explanation under it (with nothing sealed, the safe playbook is filed), and its wording "your
  orders are final for this Lull" (`godot/scripts/strings.gd` ~133, ~174) reworded to state the
  rig's own gate (after Ready this client sends no planning call), not a gateway rule; the editor's
  S3/S6 markers re-dated to S6 (`editor.rs` ~594, `editor.gd` ~21 and ~107).
- **Implements:** items 139 (2) D, H, I, M, N; 143 (3), (4); register S3-02, S3-16 (re-dated).
  **Needs:** nothing new on the wire. **Acceptance:** `crates/client-gdext/tests/no_arithmetic.rs`
  and `no_sim.rs` green; the screenshot goldens re-blessed with the panel's new layout, named; the
  headless watch check run by hand. **Contract PR:** no. **PLACEHOLDERs:** none. **Agent-days:** 4.

### `con6` — the second `proto/**` pull request: the rule list's wire, predicates and `toward`

- **Owns:** as `con5`. **Branch** `feat/proto-s3-vocabulary`.
- **Builds** (decision 2's recommended set; a cut item stays reserved, its comment naming S6):
  - **Predicates:** own beacon 43 to 45 (units by role, mandate, has a finished structure of a kind);
    economy 72 and 73 (kW supply, kW draw); enemy knowledge 60 to 62 (enemy units near a location,
    an enemy beacon known, an enemy structure near), each carrying a required `Freshness`
    (`VISIBLE_NOW` or `KNOWN_WITHIN` ms, spec §10), the blocks' remaining numbers still reserved with
    their stage; U-07's stub naming what S3 fills and what S6 does.
  - **`toward`:** `Location` 12, `toward { FeatureRef feature }`, its comment giving E23's reading.
  - **The rule list's reads:** `RenderPlanResponse`'s chips (E8), and `get_vocabulary`'s request and
    response with its `locked` reason; `GetBeaconResponse`'s per-beacon detail for the pickers (X-17:
    mandate, settings rows present, Build targets); `EstimateRouteResponse`'s rendered travel text
    (S3-18, items 57 and 61); the guided pick's mark and reason on the template list's answer (E25).
  - M-12's and M-13's markers reworded as decision 8 rules.
- **Implements:** register U-07, X-17, S3-18 (wire), M-12, M-13; S1's decision 3 (`toward`).
  **Needs:** `con5` merged; decision 2. **Acceptance:** as `con5`; the sim refuses every new
  predicate and `toward` with `NotAtThisStage("S3")` until `pred` merges, and the verifier with its
  E-code, so nothing is half-usable (E8 of S2's plan). **Contract PR:** yes. **Moves:** the schema
  and docs goldens; no chain. **PLACEHOLDERs:** none. **Agent-days:** 3.5.

### `vfy5` — `crates/verifier`: the licence, the Spire and the scope

- **Owns:** `crates/verifier`. **Branch** `feat/verifier-licence`.
- **Builds:** for `resonance_spire` alone, E0507 (a `queue_structure` row for it when the frozen
  snapshot's seat does not hold the licence) and E4's one-Spire-per-beacon E-code, built and tested
  behind E0009, which still refuses every `queue_structure` row (`refusals.rs` ~85) until `lic`
  narrows it to the other blueprints, so the verdict never runs ahead of the sim's compile; E0506
  stays unemitted (E4); `Scope`'s per-beacon sphere radius through its own setter (E3), and its held
  licences and sighted enemy beacons (S3-04's field, which `gwr` fills), each a `report_hash` input,
  with `KnownBeacon`'s fields unchanged; the I-level note for an Attack beacon with
  `breach_allowed` on `RETURN_FIRE`, that a breach then gets no mortar (item 151 (2) (c) (4)); W0702
  emitted by the lint stage (a rule that can never fire because an earlier rule's condition is true
  whenever the later's is; the conservative test: the later condition implies the earlier, read
  from the same predicates and constants, never by modelling play, AGENTS.md §3 rule 2); S3-19's
  marker re-dated to S6; `VERIFIER_VERSION` `0.1.0-s3.1`.
- **Implements:** item 151 (2) (c) (4); 153 (3)'s plan-time half; register S3-04 (scope). **Needs:**
  `con5` merged. **Acceptance:** each new code has its case and its catalogue row; the catalogue's
  `Emitter::NoneYet` rows for E0507 and W0702 lifted; `the_verifier_never_ranks` green; `KnownBeacon`
  literal sites in the gateway, `gamectl` and `bench` untouched. **Contract PR:** yes
  (`report_hash`'s `Scope` encoding). **Moves:** every `report_hash` golden. **PLACEHOLDERs:** none.
  **Agent-days:** 3.5.

### `lic` — `crates/sim`: the licence mechanism and the Resonance Spire

- **Owns:** `crates/sim`. **Branch** `feat/sim-licence`.
- **Builds:** E2's licence table, its settlement check (decision 6's reading of "running Generators
  on 2 vents"), its voiding on elimination, `licence_awarded` and `licence_voided`; E4's
  `queue_structure` for the Spire at any own beacon whatever its mandate, built by that beacon's
  fabricator through the Quartermaster, refused by `spire_present` when one stands; E3's derived
  sphere radius, every reader decision 6 names as growing moved to the one function and every
  reader it names as fixed left on the row by name; on the Spire's destruction the radius returns
  to its base on the same tick, with decision 6's rule for what then lies outside; S3-10's rows read
  in place of `LONG_RUN` and `SMOOTH_LOOKAHEAD`; `combat.charge_damage_hp` read, the charge's blast
  now damaging units and structures in its ball (item 151 (2) (c) (3)); the refusal table; two
  scenarios under `scenarios/s3/`: `resonance-spire` (on decision 13's seed: stepping stones,
  Generators on two vents, the licence at a settlement, the queue, the growth, a target beyond the
  old radius built the round after) and `spire-lost` (staged by its two playbooks: the other seat's
  Attack destroys the Spire, the radius returns, the event log shows it).
- **Named places, sequenced (B3 of the reviews).** The two event kinds and `spire_present` need
  their arms in the gateway's exhaustive `audience_of` (`crates/gateway/src/surface.rs` ~2104) and
  `event_text` for the branch to build. `gwp` owns the gateway while `lic` runs, so `lic` makes those
  arms as its last commit **after `gwp` merges**, merging `main` in first; likewise the verifier's
  E0009 narrowing after `vfy5` merges, and the gateway's `mandate_and_rule_gaps()` row. `lic` merges
  after `gwp`, and `gwr` starts after `lic`.
- **Implements:** item 153 (3); register S3-10 (sim half). **Needs:** `con5`, `sim3a` merged;
  decisions 6 and 13. **Acceptance:** §4.8's three places for the licence table; a save/restore
  round-trip with a licence held; a chain that earns nothing does not move (E2); the radius
  readers' test (E3); `allocations.rs` unchanged. **Contract PR:** yes (the state hash,
  `SNAPSHOT_VERSION` 15, the event vocabulary, the `StepFailure` enum). **Moves:** chains of
  scenarios whose seats run Generators on two vents at a settlement or use a charge; every
  `report_hash` golden (snapshot 15). **PLACEHOLDERs:** none beyond `con5`'s rows.
  **Agent-days:** 7. **Split seam:** the licence table and its check first, the Spire's queue and
  sphere second, the charge's damage third.

### `gwp` — `crates/gateway`: Probation, the memos and the save

- **Owns:** `crates/gateway`, and as a named place in its own PR (E6) `godot/scripts/host_link.gd`'s
  `config_line` and its pins in `crates/client-gdext/tests/godot_project.rs`. It runs when no client
  lane is open: after `ed3a` merges, before `ed3b` starts. **Branch** `feat/gateway-probation`.
- **Builds:** E6's `preset` on the config line, with the lobby writing it in the same PR, and, under
  decision 13, `host_link.gd`'s `MATCH_SEED` set to `sim3a`'s Probation seed (the watch check's own
  seed unchanged); E7's
  untimed Lulls and the refusal of a reported countdown in a Probation match; E5's memos for
  decision 4's power-shortfall, dark-Stake and first-death triggers, on the feed and the recap, their
  copy in `strings.rs` (the first-licence memo is `gwr`'s, once `lic`'s event kind exists); E1's
  handle table and E5's issued-memo set in the save (`SAVE_FORMAT_VERSION` 3), the regression test
  first; E25's guided pick (the template list's answer in a Probation match's first Lull marks the
  template to start with and carries its reason, from the advisor's suggestion); E18's seed
  withheld from seat tokens; strings E (the fallback's internal names "Shadow" and "Hold" in the
  event list replaced by the player's words), F (steps numbered from 1 in the event list, as the
  route numbers them), item 143 (3)'s first (after the match ends a refused ask no longer says
  planning opens in the next Lull), and item 143 (4)'s `safe_playbook_filed` line (`strings.rs`
  ~460), which says "The Lull ran out" even when Ready ended it, reworded to say which ended it;
  the markers it owns re-dated (`viewfeed.rs` ~63 to S4, `crates/gateway/src/surface/knowledge.rs`
  ~677 to S6).
- **Implements:** items 152 (1) 4, 153 (2); E1, E5–E7, E18, E25; register S1-40 (gateway half);
  items 139 (2) E, F; 143 (3), (4). **Needs:** `con5`, `ed3a` and `sim3a` merged; decisions 4, 5 and 13.
  **Acceptance:** a resumed match names its `e_NN` as before (the test written first and red on
  `main`); a version-2 save refused with both versions named; each memo fires once, survives a
  resume without repeating, and never fires in a `custom` match; a countdown reported in a Probation
  match refused; the lobby's line and the gateway's reader agree in the same PR, and the headless
  watch check run by hand on the branch. **Contract PR:** yes (the save container, the config line
  the lobby remembers). **Moves:** the gateway's recap, walkthrough and map-summary goldens.
  **PLACEHOLDERs:** the memo copy, `PLACEHOLDER: the memo's wording — owner, at S3's demo` (spec
  §19: "Tuning S3, polished in S6"). **Agent-days:** 4.5.

### `bench3` — `crates/bench` + `xtask`: the P3 and P1 FULL harnesses

- **Owns:** `crates/bench`; `xtask/src/main.rs`'s `perf_alarms` entries and two new commands,
  `p3-budget` and `p1-budget` (in a new `xtask/src/budget.rs` beside `g3.rs`). **Branch**
  `feat/bench-p1-p3`.
- **Builds:**
  - **P3** (section 5.1): repath p99 of a single unit's query on the sim's own graph at cluster 32,
    cold, with the tick's repair share reported beside it and never inside it (item 60); the
    estimate's wall time through the gateway's `estimate_route` in a hosted Lull (`hosted.rs`'s
    match) for a one-leg route (a single from–to pair), with a five-leg route's figure reported
    beside it, and through the sim's estimator alone; on three seeds, the golden seed first;
    nearest-rank percentiles from `stats.rs`.
  - **P1 FULL** (section 5.2): QUICK and FULL p50 and p99 over the committed verifier cases and over
    generated at-budget playbooks of several shapes (all handlers with depth-4 conditions, all
    Build targets, all protected areas, the longest route) at 64, 96, 128, 192 and 256 units (E24);
    the decision tick's cost per size unit (S1's `decision.rs`) at the same sizes, **in nanoseconds
    and in counted evaluation units**, the count added to `pharmakos-bench`'s notice (item 139 (5):
    "count carried to S3").
  - **The G3′ load with raids** (E13): the gate's layout with Attack beacons and their launch
    settings, and a variant with every seat's playbook at size B (E24's test (c)); `cert3` re-runs
    both.
  - `perf-alarms` gains the figures as notices (one step per alarm, GitHub's ten-notice limit);
    `p3-budget` and `p1-budget` fail at E12's doubled numbers or on a missing verdict;
    `tests/harness.rs`' list of restated items extended.
- **Implements:** spec §16's G2+P3 and P1 rows; items 139 (5), 152 (1) 10; E11–E13, E24. **Needs:**
  `sim3a` merged (the seam), and `vfy5` (the verifier it times). **Acceptance:** `wall-guard` and
  the research guard green; every figure reproducible from its seed and `rules_hash`; notices on all
  three runners. **Contract PR:** yes (`xtask`). **PLACEHOLDERs:** none (the thresholds are the
  spec's). **Agent-days:** 5. **Split seam:** P3 first, P1 FULL second, the G3′ load third.

### `gwr` — `crates/gateway` + `crates/plan-core`: the rule list's reads and the licences on the wire

- **Owns:** `crates/gateway`, `crates/plan-core`. **Branch** `feat/gateway-rule-list`.
- **Builds:** E8's chips in `render.rs`, with their own golden under `tests/golden/plan-core/`;
  `get_vocabulary` (scope `docs`), every entry from the schema and the rules table, its locks from
  the seat's licences; `get_beacon`'s detail (X-17); `estimate_route`'s rendered travel text
  (S3-18); the licences on the wire (`GetRecapResponse.licences_awarded`, the briefing's held
  licences); `BeaconSummary.sphere_radius_voxels` from the sim's one function (E3); the verifier
  scope's sighted beacons (S3-04) and its per-beacon radii through `Scope`'s setter; the
  first-licence memo (E5); E22's sweep over both crates.
- **Implements:** E3 (gateway half), E5 (the fourth memo), E8; register X-17, S3-04, S3-18 (gateway
  half). **Needs:** `con6`, `vfy5` and `lic` merged. **Acceptance:** `crates/gateway/tests/methods.rs`
  pins each new answer; `confinement.rs` green (no handler steps or seals); the chips' golden
  human-readable; the fog rule on the recap's licence line held by a test. **Contract PR:** no beyond
  the goldens (the wire is `con5`'s and `con6`'s). **Moves:** the gateway's and plan-core's render
  goldens gain chips; the scope's content moves the gateway's `report_hash` goldens.
  **PLACEHOLDERs:** none. **Agent-days:** 5.

### `pred` — `crates/sim`: the predicates, `toward` and Survey's re-scan

- **Owns:** `crates/sim`. **Branch** `feat/sim-predicates`.
- **Builds:** each `con6` predicate's evaluation in the interpreter, reading the seat's knowledge
  only (the enemy predicates the sighting table, with its freshness); `toward`'s site (E23);
  Survey's re-scan as decision 7 rules (`survey.rs` ~135), with the per-cell explore time it needs
  as a hashed, snapshotted column (`SNAPSHOT_VERSION` 16); the refusal-table lifts, with the
  verifier's E-codes left for `vfy6`; two scenarios on decision 13's seed: `toward-stepping-stone` (a
  beacon placed toward a far vent in round 1, from it the vent covered in round 2) and
  `enemy-known-rule` (a Defend handler fires on `enemy_units_near` with a fresh sighting and not on
  a stale one).
- **Implements:** register U-07 (sim half), S1's decision 3; item 152 (1) 6. **Needs:** `con6`, `lic`
  merged; decisions 2, 7 and 13. **Acceptance:** each predicate has an interpreter transcript case;
  no committed scenario's chain moves except where the re-scan changes a Survey beacon's scouting; a
  predicate never reads another seat's state (a confinement test); §4.8's three places for the
  explore-time column. **Contract PR:** yes (the state hash and `SNAPSHOT_VERSION` 16, under
  decision 7's recommendation; no, if the owner keeps "never re-scanned"). **PLACEHOLDERs:** none
  (the re-scan's clock is reach memory's row). **Agent-days:** 5.5. **Split seam:** the predicates
  first, `toward` second, the re-scan third.

### `nite3` — `crates/gamectl`: the Rusher's guard, the hunts count and `gamectl verify`

- **Owns:** `crates/gamectl`, `scenarios/nightly/`. **Branch** `feat/gamectl-nightly-guard`.
- **Builds:** E14's two fields and the Rusher's round-2 launch test; E19's view line on `gamectl
  verify`; `turtle.playbook.jsonc`'s marker re-dated to S5 (item 152 (1) 3); E22's sweep over
  `crates/gamectl`.
- **Implements:** items 150 (2), 151 (4), 152 (1) 3, (3); register S3-03. **Needs:** `con5` merged
  (its `rules_hash` already in the result golden). It does not wait for `opr3`: the result golden
  moves with each lane that changes it (`con5`, `nite3`, `opr3`, `tune3`), each re-bless naming its
  cause. **Acceptance:** the nightly result golden re-blessed, its new keys named; the Rusher's test
  red on a Rusher that never launches (shown in the PR by a probe). **Contract PR:** yes (the
  nightly result golden's format, additively). **PLACEHOLDERs:** none. **Agent-days:** 2.5.

### `ed3b` — `crates/client-gdext` + `godot/`: the rule list

- **Owns:** `crates/client-gdext`, `godot/`. **Branch** `feat/client-rule-list`.
- **Builds:** `rule_list.gd` from read-only rows to the spec's rule list (§13): each rule a sentence
  with clickable chips drawn from `render_plan`'s spans; a chip's editor by its kind (a number field
  within the bounds sent, an enum's values, a reference picked on the map), each edit one JSON Patch
  `replace` sent to `patch_plan`; drag reordering as a `move`; the conflict connector between the
  two rules a W0702 row names; the condition picker, family → predicate → parameters, drawn from
  `get_vocabulary`, its locked entries greyed with their reason; S3-01's Visit & change rows for a
  beacon's mandate and Build targets, from `get_beacon`'s detail (cut 1); S3-18's rendered travel
  figure drawn in place of the raw milliseconds (`strings.gd` ~183); and finding C (item 139 (2),
  (5)): a client test that Alt-click on a vent composes the description, `covering {vent {NEAREST,
  UNCOVERED}}`, with the owner's hand check at P6a and any failure fixed here. Nothing here
  validates, ranks or computes (rule 4); the main session's docs PR names the new pieces in
  AGENTS.md §3 rule 4.
- **Implements:** register S3-17, S3-01, S3-18 (client half); item 139 (2) C. **Needs:** `gwr` and
  `gwp` merged. **Acceptance:** the client's tests pin each patch a chip, a drag and a pick compose;
  a screenshot golden of the rule list; the headless watch check by hand. **Contract PR:** no.
  **PLACEHOLDERs:** none. **Agent-days:** 8. **Split seam:** chips, drag and the connector first;
  the pickers, locks and Visit & change rows second.

### `vfy6` — `crates/verifier`: the predicates' checks and `toward`

- **Owns:** `crates/verifier`. **Branch** `feat/verifier-predicates`.
- **Builds:** each new predicate's structure and semantics checks; E0209 emitted (an enemy
  predicate without its freshness; its catalogue row lifted from `NoneYet`); `toward`'s resolve
  check (a named feature the seat can name); decision 8's I0003 notes for an absent `on_fail` and a
  zero `max_deaths_before_fallback`; the fuzz generator (`tests/fuzz.rs`) extended to the new
  constructs and to sizes at and past 256 units, so P1's "10k fuzzed playbooks" covers any budget
  `tune3` can choose; S3-15's marker closed (the spec's limits are technical); `VERIFIER_VERSION`
  `0.1.0-s3.2`.
- **Implements:** register U-07 (verifier half), S3-08, S3-09, S3-15; E0209. **Needs:** `pred`
  merged. **Acceptance:** every new construct's verdict agrees with the sim's compile (S2's E8
  test); `the_verifier_never_ranks` green. **Contract PR:** yes (`VERIFIER_VERSION` moves every
  `report_hash`). **PLACEHOLDERs:** none. **Agent-days:** 2.5.

### `opr3` — `crates/operator` + `library/`: Easy in S3

- **Owns:** `crates/operator`, `library/`, and as named places (AGENTS.md §6) the tests and goldens
  that pin Easy's output: `crates/gamectl/tests/operator.rs`, `operator_host.rs`, `wizard.rs`,
  `crates/gateway/tests/advice.rs`, the gateway's `instantiate_suggested` golden, and
  `tests/golden/scenarios/nightly-result/`. Its two client-side pins,
  `crates/client-gdext/tests/wizard.rs` and `godot/fixtures/instantiate_suggested.json`, sit in
  crates `ed3b` owns, so they are **its last commit, made after `ed3b` merges**, with `main` merged
  in first; `opr3` merges before `prob` starts (AGENTS.md §6). **Branch** `feat/operator-s3`.
- **Builds:** Easy queues the Spire at its core once its seat holds the licence (the capability
  weight it already has; `candidates.rs` ~101's comment rewritten); Easy's stepping stones with
  `toward` (S5-04); under decision 3, Easy's Survey post on spec §14's trigger, "when the enemy
  sightings it needs are stale or missing", so its standing raid post launches once its scouts have
  refreshed them, with S5-14's module-doc marker reworded to what is left (chatter, target spread);
  Hold & Build writing its two omitted settings so it draws no I0003 note (finding L); the repair
  threshold's marker in `library/hold_and_build.jsonc`, the scenario copy and the fixture re-dated
  to S3's demo; `situation.rs` ~619 re-dated to S5; S1-52's marker re-dated (section 2.1).
- **Implements:** register S5-04, S5-14 (the Survey post), S1-52 (re-dated); decision 3; item 139
  (2) L. **Needs:** `pred`, `gwr` merged; decisions 2 and 3; `ed3b` merged for its last commit.
  **Acceptance:** every template and Easy's output qualify under the verifier; the nightly harness
  forced once on the branch with the three arms' figures in the PR. **Contract PR:** yes
  (`tests/golden/package/`'s manifests if `library/` changes a file name; otherwise no).
  **Moves:** `against-easy*` chains, the operator's and `demo_*` goldens, the nightly result golden.
  **PLACEHOLDERs:** none (the Survey post's trigger is the spec's). **Agent-days:** 4 under decision
  3's recommendation (3 without the Survey post).

### `cert3` — the certifying runs and the alarm job

- **Owns:** `crates/bench` (from `bench3`), `xtask/src/budget.rs`, `.github/workflows/ci.yml`'s new
  `authoring budgets (ubuntu-24.04)` job and the cross-OS guard's list of verifier reports, and
  `nightly-scenarios.yml`'s `not-yet-enabled` summary text (item 151 (4)). **Branch**
  `feat/bench-certify-s3`.
- **Builds:** the recorded runs of section 5 (the owner runs them on the i7-9800X, Windows and WSL2,
  three repetitions; the lane prepares the commands and the report's form); `bench3`'s G3′ load with
  raids re-run (E13) and the load at size B (E24); E15's view-bytes and recompute notices; the peak
  sightings per seat at the gate load (E17); the breach search's cost at the gate load (`attack.rs`
  ~140); S1-49's seed kept or a second added (E12); the job.
- **Implements:** spec §16's G2+P3 and P1 FULL certification; items 152 (1) 10, 11; register S1-49,
  S3-11 (reported). **Needs:** `vfy6`, `pred`, `bench3` merged (the final verifier and sim).
  **Acceptance:** the report's every figure reproducible from its seed, `rules_hash` and commit; the
  job green on the branch. **Contract PR:** yes (`.github/workflows/**`, `xtask`).
  **PLACEHOLDERs:** none. **Agent-days:** 2.

### `prob` — `crates/client-gdext` + `godot/`: Probation and Playtest mode

- **Owns:** `crates/client-gdext`, `godot/`. **Branch** `feat/client-probation`.
- **Builds:** the lobby's one match named Probation (the config line's `preset` already written by
  `gwp`), the older-line refusal answered by a fresh match; the guided first pick (decision 5) drawn
  from the gateway's mark and reason (E25), with no template named in GDScript; memos drawn in the
  event list and the recap as the Ledger's, from the `ledger_memo` kind; the Spire and the larger
  sphere ring drawn plainly (`vista.gd`: a marker and a ring at the served radius; the lattice and
  the aurora are S6's art pass); the licence line on the recap; E10's Playtest mode, survey, client
  diagnostics log and bundle, with E10's extended no-network check over `crates/client-gdext` and
  `godot/`; S1-40's marker reworded to its S6 half (the settings screen).
- **Implements:** items 153 (1), (2); register S1-40 (client half). **Needs:** `gwp`, `gwr`, `ed3b`
  and `opr3` merged; decisions 4, 5 and 11. **Acceptance:** E10's entry-list, log-shape and
  no-network tests; `crates/client-gdext/tests/wizard.rs`' `the_client_names_no_template` still
  green; the watch check by hand; the main session's local package run. **Contract PR:** no (the
  line's format is `gwp`'s). **PLACEHOLDERs:** none beyond `gwp`'s copy. **Agent-days:** 4.
  **Split seam:** the preset, guided pick and memos first; Playtest mode second.

### `tune3` — the size budget, the rooms and the re-read rows

- **Owns:** `rules/rules.v1.json`, `rules/README.md`, `proto/gp/v1/rules.proto`'s comments (the
  third `proto/**` PR, E21), and as named places the constants E17 lists in `crates/sim/src/world.rs`
  and `interpreter.rs` and the budget's reader in `crates/verifier/src/limits.rs` (~175–184), while
  no lane owns those crates; the balance check `crates/gamectl/tests/balance.rs`; the nightly result
  golden's re-bless. **Branch** `feat/rules-s3-tune`.
- **Builds:** `verifier.size_budget_units` set by E24's mechanics and decision 9's direction; E17's
  rooms; decision 6's Spire numbers and the charge's damage from the duel and balance reports; the
  eight delegated S3 rows reported (section 2.1); rules revision 8; every marker re-dated to S3's
  demo or closed. **The S2 rows and the economy, power and map rows, by decision 3's answer:** if
  Easy fights, each is re-read with the balance check re-run on S3's rules against that Easy, and
  kept or moved with the report line that bears on it; **if decision 3 is "No"**, no re-read is
  possible against a passive Easy, so each row is kept at revision 6's value and its marker
  re-dated to "owner, at S5, with Normal and Hard, once Easy fights", the balance report published
  for the record only.
- **Implements:** item 152 (1) 1, 2; spec §19's S3 rows; register S3-06, S3-12, S3-13, S2-19 and the
  delegated rows. **Needs:** `cert3`, `opr3`, `nite3` merged. **Acceptance:** every template and
  committed playbook fits the new budget (a test); P1 FULL's p99 at the new budget re-read from
  `cert3`'s curve. **Contract PR:** yes (`rules_hash`, `rules.proto`). **Moves:** every chain, the
  determinism chain, every `report_hash` golden, the nightly result golden. **PLACEHOLDERs:** each
  kept row's marker re-dated. **Agent-days:** 4.

### `demo3` — the run sheet, the certification on final rules and the register

- **Owns:** `docs/demo/s3-run-sheet.md`, `docs/demo/fun-gate-sheet.md`, `docs/placeholders.md`'s S3
  rows. **Branch** `docs/s3-demo`.
- **Builds:** section 1.1's run sheet with its staged beats said to be staged; section 1.4's
  readiness list checked; the fun gate's tester sheet; P1 FULL and G2 + P3 re-run on `tune3`'s rules
  (section 5's last steps) and recorded; the register's S3 rows closed or moved with their owners;
  the decisions-log item handing S3 to its demo.
- **Implements:** AGENTS.md §10 items 7 and 8. **Needs:** every other lane merged. **Contract PR:**
  yes (design docs). **Agent-days:** 2.5.

### `p6` — the owner's P6 timing sessions (main session and owner, not a lane)

Two sittings, from `docs/demo/p6-run-sheet.md`, which the main session writes in a docs PR before
`ed3a` merges. **P6a**, after `ed3a` (about day 4.5): both timings on the editor as it then stands,
the template wizard and the map route with the size meter. **P6b**, after `ed3b` (about day 24),
both timings again with the rule list. Decision 15 says which is the record. Section 5.3 gives the
protocol.

---

## 4. The schedule

### 4.1 Waves

Three slots, never more (AGENTS.md §6). `crates/sim`, `crates/gateway` and `crates/client-gdext`
each have one author at a time. Three named places cross a running owner, and each is sequenced
rather than run beside it: `lic`'s gateway arms and its verifier narrowing are its last commits,
made after `gwp` and `vfy5` merge, so `lic` merges after `gwp`; `gwp`'s lobby edit lands while no
client lane is open, between `ed3a` and `ed3b`; and `opr3`'s client pins are its last commit, made
after `ed3b` merges, so `opr3` merges before `prob` starts. A lane waiting on such a merge holds its
PR open and no agent. Days are working days in plan units, five to a week, at each task's midpoint;
a task starts the day its inputs merge. The bulk answer falls before wave 0: every lane's Needs line
names the decisions it builds on.

| Wave | Days | Slot 1 — sim, then operator and rules | Slot 2 — proto, gateway, verifier | Slot 3 — client, verifier, bench | The owner can see |
|---|---|---|---|---|---|
| 0 | 0–4.5 | **sim3a** (2) → **lic** starts (3) | **con5** (3) → **con6** starts (3) | **redate3** (0.5) → **ed3a** (4) | the re-dated markers; the size meter; P6a at day 4.5 |
| 1 | 3–11 | **lic** (7, to 10; merges at 11, after `gwp`) | **con6** (to 6.5) → **gwp** (4.5, from 6.5) | **vfy5** (3.5, from 4.5) → **bench3** starts (8) | Probation's preset and memos headless; the licence and the Spire headless |
| 2 | 11–24.5 | **pred** (5.5, from 11) → **opr3** (4, from 16.5; merges at 24.5, after `ed3b`) | **gwr** (5, from 11) → **vfy6** (2.5, from 16.5) → **cert3** (2, from 19) | **bench3** (5, to 13) → **nite3** (2.5, from 13) → **ed3b** (8, from 16) | the rule list; the P3 and P1 reports; P6b at day 24 |
| 3 | 24.5–31 | **tune3** (4, from 24.5) → **demo3** (2.5, from 28.5) | — | **prob** (4, from 24.5) | Playtest mode; the S3 demo of §1.1 |

**Totals.** 19 tasks, **about 74.5 ad** (65–84), including about 1.5 ad of re-blesses after merges
of `main`; 73.5 without decision 3's Survey post. **Two critical paths of about 31 working days:**
the client's `con5` → `con6` → `gwp` → `gwr` → `ed3b` → (`opr3`'s merge) → `prob` → `demo3`, and
the sim's `sim3a` → `lic` (waiting a day for `gwp`) → `pred` → `opr3` (waiting for `ed3b`) →
`tune3` → `demo3`, whose own work is about 27 days and whose two waits make it equal. Slot 2 is idle
from day 21 by dependency. Slot utilisation about 80 % of 93 slot-days.

**Token cost.** At S2's sub-agent rates (about 186–245 k tokens per planned agent-day, item 115 (2)
and S2's waves), 74.5 ad is about **14–18 M** tokens.

### 4.2 The re-bless ledger: how many times the chains move, and why

| PR | Scenario chains | Determinism chain | `report_hash` goldens | Other goldens |
|---|---|---|---|---|
| `con5` (rows nothing reads yet) | — | — | all (`rules_hash`) | the nightly result (its `rules_hash`) |
| `ed3a` | — | — | — | the client's screenshots |
| `con6` | — | — | — | the schema and docs |
| `vfy5` (`VERIFIER_VERSION` 0.1.0-s3.1; `Scope`'s new fields) | — | — | all | — |
| `gwp` (the save, the preset, the seed withheld) | — | — | — | the gateway's recap, walkthrough and map summary |
| `lic` (the licence table, empty until earned; the charge's damage; S3-10's rows at their constants) | its two new ones; any whose seats run Generators on two vents at a settlement; any with a charge (`atk`'s) | — expected (the determinism playbook places one Generator) | all (snapshot 15) | — |
| `gwr` (chips; the scope's sightings and radii) | — | — | the gateway's | the render goldens gain chips |
| `nite3` | — | — | — | the nightly result (new keys) |
| `pred` (snapshot 16 under decision 7) | its two new ones; a Survey beacon's where the re-scan changes its scouting | — expected | all (snapshot 16) | — |
| `vfy6` (0.1.0-s3.2) | — | — | all | — |
| `opr3` (Easy's output; Hold & Build's settings written) | `against-easy*` | — | the operator's and the `demo_*` | the nightly result; the wizard pins |
| `tune3` (the budget, the rooms, the re-read rows) | all | **moved** | all (`rules_hash`) | the nightly result |

Over the stage the scenario chains move twice for certain (`lic`'s, `tune3`) and up to twice more
(`pred`, `opr3`); **the determinism chain once** (`tune3`); every `report_hash` golden **six times**
(`con5`, `vfy5`, `lic`, `pred`, `vfy6`, `tune3`) and the gateway's a seventh (`gwr`); the nightly
result golden four times (`con5`, `nite3`, `opr3`, `tune3`), each naming its cause. Where two
running lanes move one set, the second to merge re-blesses after its merge of `main` and names the
lane it followed.

### 4.3 Honest size

About 74.5 ad and a 31-day critical path, against the spec's 5 weeks (25 days); with a week of float
for contract review, about **7 plan-weeks**. S1's plan sized its row about 40 % over the spec's;
S3's is about 40 % over too, nearly all of it the carries (the licence mechanism moved in from S4 by
item 153 (3), about 4 ad; Probation moved in from S6 by item 153 (2), about 5 ad; the predicates and
`toward`, about 6 ad; the Rusher's guard, the `e_NN` fix and the gate-load seam, about 3 ad). The
row's own items (rule list, size meter, Save and Load, the two gates) are about 30 ad. The two waits
of section 4.1, which keep one crate one agent, cost about 4.5 days of path.

Plan units are not calendar days. S1's 71.5 ad ran 17 calendar days (2026-09-29 to 10-16); S2's
128 ad ran 12 (10-16 to 10-28). The calendar is set by review and by the three-OS matrix that most
S3 lanes pay (`scripts/merge-train.sh`'s `MATRIX_ONLY` paths: `crates/sim/`, `crates/client-gdext/`,
`godot/`, `library/`, `rules/`, `.github/`).

### 4.4 How lanes run in S3

- **`wave-lanes.js` is pointed at this plan** by `args.plan` = `docs/design/s3-plan.md` and
  `args.stage` = 'S3'; the task headings carry their ids in backticks. The environment's sentence on
  `cargo xtask determinism --bless` names `tune3` alone.
- **Briefs** carry: the operator's global rules on errors and the no-personal-data rule; the lane's
  decisions as ruled; `CARGO_TARGET_DIR` on every cargo and commit command; a commit-message file of
  the lane's own (item 151 (3)); and, for `lic`, `gwp` and `opr3`, the sequenced last commit of
  section 4.1.
- **The main session's docs PRs** carry, beside the rulings: spec §3's Probation paragraph (the
  memos as decision 4 rules, untimed Lulls by preset, the map as decision 13 rules), §8 (one licence
  from S3, the mechanism general, decision 6's readings), §13 (the chips, the vocabulary read, the
  meter), §14 (Easy's Survey post, if decision 3), §16's fun-gate row (its "Owner … criteria frozen
  before S3 starts" discharged by the owner's word, and its **"Go if …" cell replaced by "the
  owner's judgement, on the playtest bundle's four metrics (decisions-log item 153 (1))"**), §3's and
  §16's "ships publicly as v0.1" and §17's Gate + v0.1 row as decision 10 rules, §17's S3 and S4
  rows (the licence mechanism moved; decision 2's moves), §19 (the memo copy "Tuning S3, polished in
  S6"); AGENTS.md §3 rule 4's new client pieces (the meter's ratio, the chips' drawing and the drag's
  `move`, the pickers, the guided pick's mark drawn as sent, the memos drawn, Playtest mode's
  counters and its diagnostics log), §7's playtest sentence, §9 item 11 (the authoring alarm job);
  `docs/placeholders.md`'s S3 rows; `docs/design/README.md`'s row for this plan;
  `docs/demo/p6-run-sheet.md` before `ed3a` merges; `scripts/merge-train.sh` stopping on the
  authoring alarm (E12).
- **Packaged locally before merging:** `con5` (`rules/` ships), `ed3a`, `gwp` (its lobby edit),
  `ed3b`, `opr3`, `prob`, `tune3`.

### 4.5 The cut order, written before the stage starts

If S3 runs late, these move out, in this order, and nothing else does without the owner's word. A
cut construct is never silently ignored: its proto numbers stay reserved, the sim answers
`NotAtThisStage("S6")` (or `"S5"`) and the verifier its E-code, so a file that uses it is refused
with a reason.

1. **The rule list's Visit & change rows** (S3-01, to S6), the last item of `ed3b`'s second half.
   About 1 day off the client's path, the stage's critical one. *Downside:* a beacon's mandate and
   targets are changed by editing the route's step in the rule list rather than from the map menu.
2. **The enemy-knowledge predicates and E0209** (60 to 62, to S6). About 2 ad and 1 day off the
   sim's path (`pred`, `vfy6`, `ed3b`). *Downside:* a rule can ask about its own beacons and economy
   but not about the enemy; Defend still reacts through `beacon_under_attack`.
3. **Easy's Survey post** (to S5, as planned before decision 3), if decision 3 took it. About 1 ad
   and 1 day (`opr3`). *Downside:* Easy stays passive, the combat rows rest on the duel report alone
   as at S2, and `tune3` takes decision 3's "No" branch.

**Never cut:** every item of the row (the rule list's chips, drag, connector, pickers and locks; the
size meter; Probation's preset, memos and guided pick; the Spire and its licence; Save and Load; P6,
G2 + P3 and P1 FULL), Playtest mode and the bundle (item 153 (1)), the `e_NN` fix (item 152 (1) 4),
the Rusher's guard (item 152 (3)), and, under decision 13's recommendation, **`toward`**: without
stepping stones a Probation seat never reaches a second vent, so the licence would not be earnable
in the match the gate plays (section 1.1's finding).

**Checkpoints** (decision 12): at `ed3b`'s start (planned day 16), later than day 17 takes cut 1; at
`pred`'s start (planned day 11), later than day 13 takes cut 2, since the sim's path has about two
days of its own before its waits; at `opr3`'s start (planned day 16.5), later than day 18 takes cut
3. Past the last, the main session puts the slip to the owner.

### 4.6 The contract pull requests, in merge order

Each body names its paths; each new hashed field goes into the state hash, the snapshot round-trip
and the goldens in the same PR (AGENTS.md §4.8); each snapshot change bumps `SNAPSHOT_VERSION` (14
today) and refuses an older one with its typed error; event kinds and step failures are added at the
end with new names and ids taken at the merge of `main`. Thirteen PRs, one of them conditional:

| Order | PR | Contract paths |
|---|---|---|
| 1 | `con5` | `proto/gp/v1/rules.proto`, `gateway.proto`, `playbook.proto` (comments, `gp.v1.Licence`), `seams.proto` (M-14's comment), the generated tree, `proto/buf.yaml`'s scoped `ignore_only`, `rules/rules.v1.json` (`rules_hash`), golden contents |
| 2 | `con6` | `playbook.proto` (predicates, `toward`), `gateway.proto` (chips, `get_vocabulary`, `GetBeaconResponse`, `EstimateRouteResponse`, the guided pick's fields), `proto/buf.yaml`, the generated tree, the schema and docs goldens |
| 3 | `vfy5` | `report_hash`'s `Scope` encoding; `VERIFIER_VERSION` |
| 4 | `gwp` | the save container (`SAVE_FORMAT_VERSION` 3); the config line the lobby remembers (`preset`) |
| 5 | `lic` | determinism code: the licence table in the state hash, `SNAPSHOT_VERSION` 15; the event vocabulary (`licence_awarded`, `licence_voided`); `StepFailure` (`spire_present`) |
| 6 | `bench3` | `xtask/src/main.rs`' perf-alarm list, `xtask/src/budget.rs` |
| 7 | `nite3` | the nightly result golden's format (additive) |
| 8 | `pred` | determinism code: the explore-time column, `SNAPSHOT_VERSION` 16 (under decision 7's recommendation) |
| 9 | `vfy6` | `VERIFIER_VERSION` |
| 10 | `cert3` | `.github/workflows/ci.yml` (the new job, the cross-OS guard's list), `nightly-scenarios.yml` (a summary text), `xtask` |
| 11 | `opr3` | `tests/golden/package/`'s manifests, only if a `library/` file is added or renamed (the conditional one) |
| 12 | `tune3` | the rules' values (`rules_hash`), `rules.proto`'s comments, the generated tree |
| 13 | `demo3` | `docs/design/` (this plan's "Merged as" lines, the decisions log, the handoff) |

---

## 5. The depth task and the exit gate

### 5.1 G2 + P3, the depth task

**The criterion, verbatim from spec §16.** When: "HPA\* prototype in the spike, wk 1–3; certified in
the sim at S3". Go if: "Repath p99 ≤5 ms; travel estimate ≤1 ms and within ±15% (p90) on static
terrain". Fallback: "Show ranges; favour arrival triggers". The spike passed at cluster 32 (repath
p99 1.84 ms, estimate p99 0.82 ms, |error| p90 2.6 %, never optimistic; `docs/spikes/G2-P3-pathing.md`
§9, item 57) on a toy build; S3 certifies the same three figures on the sim's own code.

**What S3 certifies, and how each is measured:**

1. **Repath p99 ≤ 5 ms.** One unit's repath is one query of the sim's router (`pathing/router.rs`)
   from a unit's cell to its goal on the generated map's cluster graph at cluster 32. The bench draws
   2 000 pairs per seed from its seeded generator on three seeds (the golden seed first), measures
   each query cold, and reports the p50, p99 and maximum. It measures again under a crater stream
   at the G3′ destruction rate, each query issued after the tick's repair. **Repair is outside the
   repath p99**: its share is reported beside the query, never inside it, since G3′ already charges
   repair to the tick (item 60: "repair, not search, is what a walker waits for").
2. **Estimate ≤ 1 ms.** The gateway's `estimate_route` handler, in process, in a hosted Lull on the
   frozen snapshot, for **a one-leg route** (one from–to pair, the same 2 000 pairs): the p99 of the
   handler's wall time, excluding the socket, and the sim estimator's own p99 beside it. A five-leg
   route's p99 is reported beside them, and is not the gate's figure.
3. **Within ±15 % at p90 on static terrain, never optimistic.** E11: a deterministic test in
   `crates/sim/tests/pathing.rs` walks real commanders through the tick over the estimated routes on
   three seeds, and asserts it on all three OSes in CI; `cert3` records the figures.
4. **Recorded runs on the owner's i7-9800X under Windows and WSL2 Linux,** three repetitions each, the
   median of the p99s, the clocks before and after; **the slower OS sets the figure**, as G3′'s did.
   The CI runners' figures are notices; `authoring budgets (ubuntu-24.04)` fails only at 10 ms and 2
   ms (E12).
5. **Re-run on `tune3`'s rules** by `demo3`, since `climb_surcharge` and the step costs may move.

**The fallback, written now, lever by lever.** Engineering levers come first and need no word:
S3-10's two rows and the cluster size for a repath miss, the corner entrance (S3-11, E16) for a
repath or accuracy miss at cluster borders, a per-Lull cache of estimates keyed by the frozen
snapshot for an estimate-latency miss. If a figure still misses, **the spec's fallback is taken in
writing, on the owner's word** (decision 18):

- **An accuracy miss: show ranges.** `estimate_route` answers a range (its estimate and an upper
  bound from the measured error at p90) and the editor draws "1 min 10 s to 1 min 30 s"; W0701
  warns against the upper bound.
- **Favour arrival triggers.** The templates' timed guards are rewritten as arrival guards
  (`step_reached`), so a playbook leans on the clock less, and the guided pick's template with them.
- **A latency miss after the cache.** The spec's fallback names no lever for latency. This plan
  recommends treating a repath-latency miss as a repath-cap question for G3′ (item 69) rather than
  taking ranges for it; that narrows the spec's fallback, so it is the owner's (decision 18).

### 5.2 P1 FULL and the size budget, the exit

**The criterion, verbatim from spec §16.** When: "QUICK at S1; FULL and the size budget are S3's exit
criterion". Go if: "QUICK ≤5 ms p99; FULL ≤50 ms p99 at the size budget; byte-identical across
operating systems; 10k fuzzed playbooks with no panic. The per-rule cost on a decision tick (one per
250 ms of game time) is measured against the 50 ms sim tick to set the playbook size budget".
Fallback: "Smaller budget; debounce".

**What S3 certifies:**

1. **QUICK and FULL p99 at the budget B,** over the committed verifier cases and `bench3`'s
   generated worst-shaped playbooks at B (every unit spent on handlers with depth-4 conditions; on
   Build targets; on protected areas; on route steps), on the owner's machine, both OSes, the slower
   deciding; with the curve E24 describes.
2. **Byte-identical across operating systems:** the verifier's `report_hash` goldens are checked on
   all three legs today; `cert3` adds the at-budget cases' reports to the cross-OS guard's set.
3. **10 k fuzzed playbooks, no panic,** in the nightly fuzz job (`crates/verifier/tests/fuzz.rs`),
   its generator extended by `vfy6` to reach sizes at and past the budget.
4. **The decision tick's cost at B,** for three seats, **in nanoseconds and in counted evaluation
   units** (S1's `decision.rs`; item 139 (5) carried the count to S3), both in `pharmakos-bench`'s
   notice.
5. **The size budget, by three tests** (E24 holds the mechanics): (a) FULL's p99 at B ≤ 50 ms; (b)
   QUICK's p99 at B ≤ 5 ms; (c) the G3′ segment, run with every seat's playbook at size B, keeps its
   tick p99 ≤ 25 ms, G3′'s own gate. **Test (c) amends the spec's wording**: §16 measures the per-rule
   cost "against the 50 ms sim tick"; this plan measures it inside the tick it runs in, against G3′'s
   p99 budget of half that tick, so the rules' cost and the rest of the tick are counted once and
   together. The amendment is the main session's docs PR's (section 4.4). Decision 9 holds the
   direction: never above 128 in S3.
6. **Re-run on the final rules** by `demo3`.

**The fallback, written now.** If FULL at 128 misses, **a smaller budget** by E24's mechanics, never
below its floor; if the floor itself misses, the owner rules (decision 18). **Debounce**: the editor
already runs FULL after 600 ms idle; a FULL miss that only typing feels is answered by a longer idle
or by FULL on submit and on demand only, on the owner's word.

### 5.3 P6, the owner's timing

**The criterion, verbatim from spec §16.** "A first-time tester builds a qualifying playbook from a
template in ≤2 min, and writes a 10-step playbook from scratch in ≤5 min. Editor typing feel
(debounced re-check, redraw) is judged here, not by P1". Fallback: "Move the editor model into Rust;
simplify". The owner stands in for the first-time tester (item 153 (4)); the downside is recorded
there and repeated in the run sheet: a pass is weaker evidence than the gate asks, and the fun gate's
testers are the first true first-time users.

**The protocol** (`docs/demo/p6-run-sheet.md`): the packaged zip, a fresh `user://`, the golden seed,
the owner at the keyboard and the main session recording. **Template:** from round 1's Lull opening
to a sealed playbook that qualifies, starting from the wizard; ≤ 2 min. **Scratch:** from an empty
playbook to a sealed, qualifying **10-step playbook** (ten route steps, as spec §16 and item 153 (4)
say) that does what the sheet's task card says (the card is shown only when the clock starts); ≤ 5
min. Recorded: both times, each refusal seen, each mis-click the owner names, and the owner's words
on typing feel. **P6a** runs after `ed3a` (about day 4.5), **P6b** after `ed3b` (about day 24);
decision 15 says which is the record.

**The fallback.** The editor's model is already largely in Rust (`crates/client-gdext/src/editor.rs`),
so the spec's first lever is spent; the second, **simplify**, is taken on the owner's word (decision
17): chips that open on the value a template suggests, map picks in place of typed coordinates,
fewer pages in the guided pick. A miss at P6a moves those into `ed3b`'s brief before it starts; a
miss at P6b goes into `prob` or a short added lane, after cut 1.

---

## 6. Owner decisions before the stage starts

Recommendation first, marked **(Recommended)**, every option with its downside. **[play]** marks a
play-shaping decision, **[risk]** a risk accepted or refused, **[policy]** a grant of authority.
Hygiene and engineering are not asked here; they are section 2.3's. Decisions 13 to 15 came from the
plan's two reviews. A lane that finds one of these wrong in the code raises it with the owner rather
than building around it.

**1. S3's size and schedule.** *Governs everything.*
- **(Recommended) Keep the whole row and the carries as section 2 places them, about 7
  plan-weeks,** with section 4.5's three cuts the only planned reductions. *Downside:* the fun gate
  moves about two plan-weeks past wk 35.5 unless absorbed; the gate's date is the owner's.
- *Hold closer to the spec's 5 weeks by taking cuts 1 to 3 now:* about 3 days off. *Downside:* Easy
  stays passive at the fun gate, rules cannot ask about the enemy, and a beacon's settings are
  changed only through the rule list.
- *Move the licence back to S4* (undoing item 153 (3)'s scope): about 4 ad. *Downside:* the Spire
  ships as a free structure or not at all, and the memo "first licence" has nothing to announce.

**2. [play] Which deferred items ride S3.** Earlier stages deferred these "to S3"; most are not in
the row. *Gates `con6`.*
- **(Recommended) In S3:** `toward`, with targeting's "nearest" (E23: least estimated travel, ties
  to the lowest anchor y, then x), never cut under decision 13's recommendation; the own-beacon
  predicates (units by role, mandate, has a finished structure); kW supply and kW draw; the
  enemy-knowledge predicates with their freshness and E0209 (cut 2); the sighted beacons in the
  verifier's scope (S3-04). **To S4, with the Sensor Spire's reveals, which are reach:** real terrain
  fog as one mask (S3-05) and what rests on it (S3-20, S3-21, S3-23, U-08), `list_known_enemies`,
  `query_area`, and the licence and mast predicates (80 to 89). **To S6, the full editor:** the rest
  of the catalogue (commander position, the clock's rest, aggregates, posture, reports since, with
  `report_kinds` and `get_reports`), `RICHEST` and grade filters, `in_reach` and enemy-known filters,
  tags (S3-07), Defend's auto-fortify and unit mix, the forecast's what-ifs and projection (S1-46),
  the briefing's payload (X-15), the segment clock (S3-16, S3-19) and the hover ghost (S3-02).
  *Downside:* the rule list's pickers offer about a third of spec §10's families at the fun gate,
  the rest greyed "from S6"; terrain stays unfogged for one more stage, as item 107 (1) accepted.
- *The whole catalogue in S3:* about 8 ad more. *Downside:* an 8.5-week stage, and the aggregates
  family has no stated meaning to build from.
- *Only what the row names:* the rule list over today's vocabulary, no new predicates or `toward`.
  *Downside:* the pickers have almost nothing to pick, and decision 13's licence is unearnable.

**3. [play] Does Easy fight in S3?** Easy keeps a standing raid post but never attacks, because it
never knows an enemy beacon: spec §14's Survey post is not built at Easy (register S5-14, item 111
A6; S2 built S5-14's Defend guard, `oprd` #144). Every S2 tuning row was moved to S3 "re-read once
Easy fights" (item 152 (1) 1). *Gates `opr3`, `tune3`.*
- **(Recommended) Yes: Easy places a Survey post on spec §14's own trigger, "when the enemy
  sightings it needs are stale or missing",** so its raid launches once its scouts have refreshed
  them; S2's combat rows and the nightly bands are re-read against an opponent that comes; chatter
  and target spread stay S5's. *Downside:* in the fun gate's Probation match Easy's raid can arrive
  by round 2 or 3, so a newcomer is hit sooner. (Cut 3 undoes it.)
- *No: Easy stays passive until S5.* `tune3` then cannot re-read S2's rows: each is kept at revision
  6's value and re-dated to S5, "once Easy fights" (section 3's `tune3`). *Downside:* the combat rows
  rest on the duel report alone for another stage, and the fun gate measures Pushes in which nothing
  comes for the tester.
- *Only outside Probation:* Easy surveys in custom matches and the nightlies, not in the preset.
  *Downside:* two Easys to tune, and the lobby hosts only Probation until S6, so no human sees the
  one that fights.

**4. [play] The memos: which triggers, and in what voice.** Spec §3 names four candidates: "a dark
Stake, first death, a power shortfall, first licence". *Gates `gwp`, `gwr`.*
- **(Recommended) All four, each once a match, in the Ledger's deadpan voice (spec §2), one or two
  sentences and one thing to try.** Read as: **power shortfall** at the seat's first brownout that
  sheds a load; **a dark Stake** when one of the seat's beacons is first destroyed (a powered-down
  beacon is the shortfall's, so the two never fire on one event); **first death** at the commander's
  first death; **first licence** at its first award. First copy, for the owner to change:
  - *Power shortfall:* "Notice of shortfall. Your draw exceeded your supply and the grid shed load in
    the order you set. Build a Generator on a vent, or recycle what you cannot feed."
  - *A dark Stake:* "Notice of loss. A Stake has gone dark. Its works stand as neutral ruins, which
    any seat's drones may salvage, and its drones re-home to your nearest beacon." (Spec §5's local
    elimination.)
  - *First death:* "Notice of absence. Your Keybearer was lost and will return at your core after
    the usual delay, or at your nearest beacon if the core is gone. Nothing you sealed changes in the
    meantime."
  - *First licence:* "Notice of licence. You ran Generators on two vents at settlement. You may now
    queue a Resonance Spire at any of your beacons."
  *Downside:* a match with a bad round 1 shows three memos in one Push.
- *Three, without the dark Stake:* *Downside:* the loss of a beacon, the hardest event to read,
  goes unexplained.
- *A plainer voice than the Ledger's:* *Downside:* the Ledger is the world's one voice the player
  meets (spec §2); a plain voice leaves no character in the newcomer's first match.

**5. [play] The guided first pick.** Spec §13: "the first Lull opens the template wizard with a
guided pick". The mark and its reason come from the gateway (E25). *Gates `gwp`, `prob`.*
- **(Recommended) Round 1 of a Probation match opens the wizard on its template list with the
  template the advisor suggests marked "Start here" and the advisor's one-line reason; the other
  templates stay offered; the wizard's pages are then the ordinary ones.** On today's advisor that
  is Hold & Build. *Downside:* one more screen than going straight to a template.
- *Straight into the suggested template's first page:* fewer clicks. *Downside:* the tester never
  sees there is a choice, and spec §16's gate counts "a wizard bug plus the guided first pick"
  together.
- *A two-way pick, Hold & Build or Expand & Mine, nothing else shown:* *Downside:* hides the Defend
  and Raid templates a curious tester may want in round 2, and names templates in the preset.

**6. [play] The Spire: the licence's reading, the larger sphere, which readers grow, and its loss.**
The spec says "Running Generators on 2 vents at a settlement", "a larger sphere for its own beacon",
and "if the spire is destroyed, the dome collapses on camera"; it gives no number and no rule for
what grows or what the shrink leaves outside. *Gates `con5`, `lic`.*
- **(Recommended)**
  - **The licence** is earned when, at the settlement, two of the seat's finished, powered
    Generators stand on two different vents.
  - **The sphere grows by half its base radius** (24 → 36 voxels at today's row) when the Spire is
    finished.
  - **What grows** is the sphere's own uses (spec §5: "the only range for physical work"): the
    mandate's area (Build, Mine, Defend's guard escort and engagement radius, Survey's area), the work
    reach of drones (build, dig, repair, salvage), the sphere's coverage of vents and seams for
    `covering`, the anchor for placing the next beacon, and the sphere's place in the sensor seam's
    sphere output. **What does not grow:** unit and structure vision (`sight.rs` ~371, its own
    `vision_radius_voxels` rows), mapgen's twice-the-sphere reach rule (`mapgen.rs` ~1001, which
    places vents against the base radius at generation), and the commander's placement range. The
    operator and the verifier read the grown radius through the gateway's view (E3).
  - **On the Spire's destruction** the radius returns to its base on that tick; what then lies
    outside the sphere stands and stays powered, but no physical work reaches it until a Spire is
    rebuilt.
  *Downside:* a seat can lose repair over a ring of its own works in one shot, which is the spec's
  "collapses on camera" made mechanical; and a grown Defend sphere guards more ground than the spec
  states.
- *A flat bonus of 8 voxels:* *Downside:* the Spire's value then shrinks as a tuned sphere grows.
- *Only the work reach grows, not the mandate's area or coverage:* *Downside:* the Spire's "larger
  sphere" is invisible to most mandates, and a second radius concept beside the sphere.
- *The sphere growing with the lattice over 60 s:* *Downside:* a sphere that changes every tick for
  a minute churns placement, the sensor seam and the editor's ghost for no play gained.
- *Outside works go neutral on the shrink:* *Downside:* a second loss rule beside local elimination,
  harsher than the spec says.

**7. [play] Survey's re-scan.** Item 152 (1) 6 left it to S3: today an explored cell is never
scanned again (`survey.rs` ~135), while the refresh pass re-checks sightings. *Gates `pred`.*
- **(Recommended) A cell is due again once it is older than reach memory N (`verifier.
  reach_memory_ms`, 180 s), and Survey's scouts take due cells only when no unexplored cell is
  left.** One clock, the reach window, so a seat's map stays as fresh as its Attack targets must be
  (item 13), and "intelligence and surveillance a huge factor" (item 144 (11)). *Downside:* a
  per-cell explore-time column (snapshot 16, `pred` a contract PR); a large map's scouts never
  finish.
- *Never re-scanned (as built):* *Downside:* a Survey beacon goes idle once its sphere is explored,
  and only the refresh pass keeps sightings fresh.
- *Re-scanned on Survey's own refresh interval (60 s):* *Downside:* a second clock beside reach
  memory, and scouts churn cells that change rarely.

**8. [play] The readings the code already makes.** Each is built and marked; the spec is silent.
- **(Recommended) Keep each, written into the rules text and, where a setting is omitted, an I0003
  note:** an absent `on_fail` reads `SKIP` (S3-08); `max_deaths_before_fallback` 0 means no limit
  (S3-09); a paid structure under a later protected area stays unfinished while the area holds and
  finishes when it is lifted (S3-22); an area is an axis-aligned box (M-12); a patrol route is a list
  of locations (M-13). *Downside:* five readings frozen into files that use them.
- *Make `on_fail` and `max_deaths_before_fallback` required:* *Downside:* every shipped template and
  Easy's output must write them, and a newcomer's first file needs two more values.

**9. [risk] The size budget's direction in S3.** E24 and section 5.2 hold how B is measured; the
call left is whether S3 may raise it. *Gates `tune3`.*
- **(Recommended) In S3, B is never raised above 128, even if the measurement allows more.**
  Lowering a budget later refuses files that fit today, raising it never does, so S3 keeps the room
  to raise once S4's mast coverage and S6's catalogue have added their per-rule cost. *Downside:* a
  player who would have used more units in S3 cannot.
- *Set it to whatever the measurement allows, up to 256:* *Downside:* a later stage that adds
  per-rule cost may have to lower it and break files.

**10. [risk] The build the fun gate plays, and when v0.1 is public.** Spec §3 and §16 say the gate
runs "on the build that also ships publicly as v0.1", and §17 releases it publicly "that week".
*Gates `demo3`.*
- **(Recommended) The testers play a private `v0.1.0-rc.1` cut from S3's last green commit after the
  owner's S3 demo review; v0.1.0 is published (unsigned zips, a tagged changelog) only after the
  owner's go verdict, from the same commit unless the gate finds a fix.** This **contradicts the
  spec's "ships publicly as v0.1"** for the gate's own build, so the main session's docs PR amends
  §3, §16 and §17 to say the gated build is published on a go (section 4.4). *Downside:* if the
  verdict is a pivot, nothing public ships that week.
- *As the spec says: the gate's build is v0.1.0, published at S3's close, before the gate:*
  *Downside:* a pivot verdict lands on a public release.
- *The testers play `main`'s tip during the gate week:* *Downside:* the build moves under the
  testers.

**11. [play] The playtest survey's other two questions, and the bundle before S7.** Spec §15: "A
3-question end-of-match survey (including 'play another round?')"; the bundle carries "survey,
metrics, recording and the diagnostics log". *Gates `prob`.*
- **(Recommended) "Did you understand why your playbook did what it did? (yes / partly / no)" and
  "What did you most want to change during a Push? (free text)"; the bundle carries no recording
  until S7 builds recordings, and its manifest says so; its diagnostics log is the client's own
  (E10).** *Downside:* the owner judges without a recording of the tester's match, and must ask the
  tester what happened.
- *A 1-to-5 enjoyment scale as the second question:* *Downside:* a number beside "play another
  round?" that the owner has chosen not to judge by (item 153 (1)).
- *Bring a recording forward from S7:* *Downside:* about 4 ad more, and a format on testers' disks
  designed under the gate's deadline.

**12. [policy] Taking a planned cut.** As S2's decision 32: item 152 (2)'s delegation covers merges,
not scope.
- **(Recommended) The owner grants the main session the authority to take section 4.5's next cut at
  its checkpoint, in the order written, recorded in the decisions log the day it is taken; any other
  cut is put to the owner.** *Downside:* a cut lands without a per-cut look.
- *Each cut put to the owner at its checkpoint:* *Downside:* the lane whose work it removes waits,
  about a day each time.

**13. [play] [risk] Can a Probation seat earn the licence?** Section 1.1's finding: on every seed
checked the second vent is 92 to 320 voxels from a core, against one beacon's 48-voxel reach; on the
golden seed it is about 230 voxels, about nine stepping stones, beyond a three-round match. Spec §8's
"Generators on 2 vents" is then unreachable in the gate's own match. *Gates `lic`, `pred`, `gwp`,
`demo3`.*
- **(Recommended) Keep spec §8's trigger and make it reachable: `toward` is never cut (section 4.5),
  and the Probation preset uses a fixed map seed of its own, chosen by a deterministic search over
  seeds (in `sim3a`, recorded in its PR; `gwp` writes it into the lobby's line) for a two-seat map
  whose second vent lies within three stepping stones of the human seat's core; the golden seed
  stays every test's fixture.** The demo plays four rounds to show the licence and the Spire live
  (section 1.1); the fun gate's three rounds make it earnable by a seat that expands, not given.
  *Downside:* the Probation map differs from the golden fixture the vista and the S1 and S2 demos
  showed, and was picked for one property; a tester who never expands never sees the licence.
- *Keep the golden seed; the licence beat leaves the live demo* and is shown from `lic`'s headless
  scenario on a staged map. *Downside:* no Probation player, tester or owner, ever earns the licence,
  and the first-licence memo never fires in the match the gate plays.
- *Lower the trigger to the start vent alone* (`licences.resonance_vents` 1, a tuning row spec §19
  dates to S4): *Downside:* every seat earns it in round 1, so it licenses nothing; the spec's "2" is
  overruled.

**14. The view's delta mode and recompute key: built in S3 or at S7?** Item 152 (1) 11 moved S2-12
and S1-34 to S3; no S3 gate reads the view's bytes or a recompute's cost. *Gates `cert3`, `gwr`.*
- **(Recommended) Measure both in S3 (`cert3`'s notices, E15) and build them at S7,** where scrub and
  recordings make the view's volume matter, with S3's figures in their markers. *Downside:* the
  fun gate's build polls the full entity list, 20–30 KB a poll at 300 units by S2-12's estimate; in a
  two-seat Probation match the count is far smaller.
- *Build the delta mode in S3:* about 3 ad (`con6`, `gwr`, `ed3b`). *Downside:* a wire mode on the
  client path, the stage's critical one, for no gate.

**15. [risk] P6: which sitting is the record?** Item 153 (4) asks for the timing "mid-S3 while the
fallback is still affordable". *Gates the `p6` sheet.*
- **(Recommended) P6a, after `ed3a` (about day 4.5), is the record; P6b, after `ed3b` (about day 24),
  confirms the rule list did not make either timing slower.** A miss at P6a leaves the whole of
  `ed3b` to take the fallback. *Downside:* the record is taken on an editor without the rule list's
  chips and pickers, which the scratch task may need.
- *P6b is the record, the owner accepting its lateness:* *Downside:* a miss at day 24 leaves only
  `prob`'s four days for the fallback, so "simplify" would delay the stage.

---

## 7. Owner decisions during the stage

**16. The authoring alarm job as a required check.** **(Recommended)** the owner adds
`authoring budgets (ubuntu-24.04)` to the ruleset's required checks after it has passed on `main`
five times running, as decision 31 had it for G3′'s. *Downside:* a shared runner's very slow day can
redden `main`. *Alternative:* it stays non-required. *Downside:* a regression that doubles a figure
merges unless someone reads the job.

**17. If P6 misses** (whichever sitting decision 15 makes the record): spec §16's fallback,
"simplify", lever by lever as section 5.3 lists, on the owner's word; the editor model is already
largely in Rust.

**18. If G2 + P3 or P1 FULL misses** after the engineering levers: section 5's written fallbacks,
each on the owner's word: ranges and arrival triggers for a P3 accuracy miss; for a repath-latency
miss, **(Recommended)** the repath cap re-derived under G3′ (item 69) rather than ranges, which
narrows the spec's fallback (section 5.1) — *alternative:* ranges and arrival triggers for any P3
miss, as the spec reads, *downside:* ranges in the editor for a cost the player never sees; and for
P1, a smaller budget down to its floor, then debounce.

**19. At S3's stage demo:** every row `tune3` kept (S2's and S3's), with its report line; each
nightly band re-read against S3's Easy; the Spire's numbers and the charge's damage; the memo copy as
first written; whether the delegation runs on into S4 (item 152 (2)); and the readiness list of
section 1.4.

**20. The fun gate itself** is outside S3: its date, its testers (recruited from the owner's own
reach, spec §16) and its verdict are the owner's alone (item 153 (1)). S3 hands over the build, the
tester's sheet and the bundle's format.

---

## 8. Deliberately left out

**Out by AGENTS.md §11:** script runtimes and any mini-language, MCP, an SDK or published API (the
vocabulary read is internal and agent-shaped, never published), AI or LLM seats, manual control,
`set_flag`, `clear_flag`, `branch` and `repeat` (a file using them is refused on Load with a code and
a pointer, as today), the expert editor view (raw JSON, command palette), networked telemetry,
translation, the art pass (the Spire's lattice and aurora), player Dispatches.

**Out because a later stage owns it.** **S4:** real terrain fog and what rests on it (decision 2),
the Sensor Spire, the Radio Mast and their licences' rows, the licence and mast predicates, the
award fund, `list_known_enemies`, `query_area`, the symmetric map. **S5:** Normal and Hard, target
spread, the bands' re-baseline, the Turtle's walls (S2-17), Easy's chatter, and its Survey post if
cut. **S6:** the settings screen (S1-40's other half), the rest of the predicate catalogue and the
filters (decision 2), the segment clock and "fits" pill, the hover ghost, the notes box and the
briefing's payload, eight templates, Save as template, the memo copy's polish, art and sound.
**S7:** the view's delta mode and recompute key (under decision 14's recommendation), recordings in
the playtest bundle, scrub. **After v1:** physics collapse, line of sight, tunnels.

**Out with no owner this stage:** `xtask/src/package.rs`'s `smoke_env` (its next owner); G1's Linux GPU
half, for want of a Linux GPU machine.

**Out on principle:** a second hash function, a shadow proto, a golden re-blessed without the rule
that moved it, a client that validates or computes, an invented rule where the spec is silent.

---

## 9. Risks, and the measurement that closes each

| # | Risk | What closes it | Where | If it does not close |
|---|---|---|---|---|
| R1 | Two critical paths of equal length, so a slip on either moves the stage | Cuts on both paths (cut 1 on the client's, cuts 2 and 3 on the sim's); checkpoints | section 4.5 | The owner's word past the last cut |
| R2 | The rule list (`ed3b`, 8 ad) is the largest client lane the project has run | Its split seam; P6a before it starts; the chips' golden and the patch tests | `ed3b` | Cut 1, then P6's "simplify" |
| R3 | The licence table or the derived sphere misses one of §4.8's places, or a reader reads the row directly | E2's three places in one PR; E3's single function and its test; a save/restore round-trip with a licence and a Spire | `lic` | The cross-OS guard and the resume tests catch it as a bisect |
| R4 | The verifier's verdict runs ahead of the sim's compile while lanes interleave | `queue_structure` and each predicate refused in both until the sim lane lifts them; S2's E8 agreement test | `vfy5`, `vfy6`, `lic`, `pred` | — |
| R5 | P6 is judged by a tester who is not new (item 153 (4)) | Said in the run sheet and the record; the fun gate's testers are the first true newcomers | section 5.3 | The gate's own "a tester who cannot file a playbook is a wizard bug" |
| R6 | The playtest bundle leaks a playbook, draft, notebook, cache file, or a path or user name | E10's entry-list test; the client's own diagnostics log and its shape test, never the raw `godot.log`; the no-network check extended to `crates/client-gdext` and `godot/` | `prob` | — |
| R7 | The save bump strands an S2 save mid-match | A version-2 save is refused with both versions named; S2 saves are development saves on the owner's disk only | `gwp` | — |
| R8 | A memo fires twice, in a custom match, or after a resume | Once-per-match tests; the issued set in the save; the preset on the config line | `gwp` | — |
| R9 | P1's budget falls below a shipped template's needs | The floor in section 5.2's rule; `tune3`'s test that every template fits | `tune3` | Decision 18's fallback, the owner's |
| R10 | P3's accuracy holds on the spike's terrain but not on the generated map's slopes | E11's three seeds; `climb_surcharge` reported against walked slopes | `cert3`, `tune3` | Section 5.1's ranges |
| R11 | Easy's raids (decision 3) make the fun gate's match feel punishing | The balance re-run's round-2 and round-3 lines; the owner at S3's demo | `tune3`, `demo3` | Cut 3, or the owner's word before the gate |
| R12 | Thirteen contract PRs (one conditional, section 4.6) queue on review | E21's one-at-a-time rule; the week of float | section 4 | Decision 1's schedule grows |
| R14 | The Probation map chosen for decision 13 plays differently from the golden fixture the earlier demos showed | The search's criterion recorded; the balance check re-run on that seed too; the owner at S3's demo | `sim3a`, `tune3`, `demo3` | Decision 13's second option |
| R13 | The certified figures differ between Windows and WSL2 by more than the codegen's 10–20 % | Both measured, the slower deciding | `cert3`, `demo3` | The slower sets the figure, the gap recorded |

---

*Sources: `docs/spec/pharmakos-spec-v0.6.html` §2, §3, §8, §10, §13–§17, §19; decisions-log §2.7
items 13, 57, 60, 63, 69, 94, 95, 107, 111, 113, 127, 128, 136, 139, 143, 144, 145, 148, 150–153;
`docs/design/s1-plan.md` and `docs/design/s2-plan.md`; `docs/placeholders.md`'s S3 section and the
S2 rows item 152 moved; `rules/README.md`'s Settled? table; `docs/spikes/G2-P3-pathing.md`; AGENTS.md
§2–§12; `cargo xtask placeholders` and the code at `3764369`.*
