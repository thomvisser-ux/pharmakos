<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# S1, the economy — plan for spec section 17, the wk-23.5 row

**Adopted 2026-09-29 under decisions-log §2.7 item 127 (11).** The main session merges this plan
after two adversarial reviews, under the delegation item 126 (2) (g) renewed for S1. The owner gets
a one-screen summary carrying item 127's answers, targeting's "nearest" and one structure per voxel,
this plan's departure from item 126 (2) (h) and targeting's size above its estimate (section 2), and
can overrule anything later. The owner rules the recommendations of sections 6 and 7 in bulk; those
marked **play-shaping** are put to the owner in an interview first (item 127 (1)). S1's first wave,
the demo's fixes, starts once this plan merges.

**Ruled 2026-09-29 (decisions-log item 128).** The owner was interviewed on decisions 1, 2, 3, 5, 6,
10 and 15, took the recommendation on each, and approved every other recommendation of sections 6
and 7 in bulk. The interview and the **[play]** marks do not match exactly: decisions 1 and 2 were
interviewed although not marked, and decisions 4, 16 and 18, marked **[play]**, were put to the
owner by name in the bulk question rather than interviewed. Every decision below therefore stands as
its **(Recommended)** option, open to the owner's overrule.

Precedence throughout: `docs/design/decisions-log.md` §2.7 > `docs/spec/pharmakos-spec-v0.6.html` >
`docs/design/co-design-gameplan-api.md` (`docs/design/README.md`); `docs/design/targeting.md` is
item 127 (12)'s adopted design and amends the spec where it says so. Where the spec is silent,
nothing is invented here: it becomes an owner decision with a recommendation and the downsides of
each option.

**Baseline.** `main` at `352e573`, where the walking skeleton is closed (item 126). T14 and T14b
built more of the row than its wording suggests: BMI with rank scaling, value following condition,
the recap's BMI settlement, the brownout and revival orders, the Quartermaster's urgency ladder,
"paid means yours" for structures and units, backlog fabrication and finite seams all exist in
`crates/sim`. S1's work is the rest: the owner's rulings, the placed beacon's `$` (X-12), the Mine
settings, the verifier's empty estimate stage, the gateway's economy surfaces, the targeting slice,
the numbers and P1.

---

## 1. Goal and end state

Spec section 17, the wk-23.5 row, verbatim: "One grid · brownout order with the beacon as the unit
of power · full Build settings · finite seams and work-in-hand fabrication · spending ("paid means
yours") · settlement at each recap · value follows condition · one catch-up dial · depth task: P1
QUICK plus the per-rule cost per decision tick." §2.7 has since added the demo's findings and X-12
(item 126 (2) (f), (h)), the spec's Lull lengths, the numbers checked headless, the grid rulings,
all four Mine settings, the targeting slice and the per-seat ids (item 127 (2), (3), (5)–(9), (12),
(13)), and S1's first contract pull request (handoff NEXT 2). It removed the lobby's settings screen
and Probation's untimed Lulls, which go to S6 (item 127 (10)).

### 1.1 The demo the owner reviews

One unsigned Windows zip (the same build on Linux) opens into a lobby, with no speed buttons and no
editor panel until a match starts. The owner starts the lobby's match: two seats, the owner and
Easy, three rounds on the 3 / 5 / 8 ladder, the golden seed. A second `Pharmakos.exe` started beside
it says it is already running rather than hanging (F3).

The first Lull runs **10:00** and every later one **5:00** (item 127 (2)); Ready still ends a Lull
at once. The wizard offers the rewritten templates, which carry descriptions, not voxels. Hold &
Build's first step reads "place a beacon covering the nearest vent you can cover"; the chip shows
"now: Heat vent (x, y) · N s — nearest by travel from the commander, read when the step starts".
Alt-click on a vent turns a named vent into that description. The editor shows E0601 and blocks
Submit until `allow_dormant_beacons` is set on a playbook that adds draw beyond supply, W0602 when
committed spending outruns the projected treasury, W0701 on a route longer than the coming segment,
and the three targeting lints with their Fix. Every click lands the first time (F1), and Save with
no file opens Save as… (F6, if it is not F1 again).

The Push: the commander walks in to the site (walk-in), `$` 60 leaves the treasury as the deploy
starts (X-12), the beacon lands, a Generator goes up `on {covered {}}`, supply rises. A deploy that
cannot finish (the commander leaves, or the site turns illegal) refunds its `$` 60 in full, and a
deploy the treasury cannot cover fails its step, which its `on_fail` takes. The Build beacon's
targets go up in their written order and nothing is built inside a protected area; while the list
has a backlog the fabricator makes build drones, and it stops once the backlog is covered
(work-in-hand fabrication). The mining drone keeps delivering in rounds 2 and 3 as the seam is dug
to its depth limit around its pillars (F2's cure), and a spent seam reads as spent. Recycling on
site refunds 50 % of the asset's remaining value, build cost × HP (value follows condition). A
deficit, made on site by recycling the beacon a Generator is homed to (decision 18), holds
fabricator orders first, then darkens domes in the brownout order with an idle expansion kept lit
(item 127 (8)); the commander walks on through the blackout (item 127 (5)), and a priority raise on
site relights a dark beacon at once (item 127 (7)).

The recap shows a settlement line (BMI and its band; the award fund named as arriving at S4), a
shortfall line when short, and why a step found nothing ("3 matched, none reachable"). A timeout's
filing says the safe playbook was filed (F5). Round 2's Lull, on the carried playbook, says "this
round: … no vent you can cover" instead of placing again at round 1's voxel (F4's cure, targeting's
map finding). Enemy beacons show as `e_NN`. The match ends on the round limit with a winner named by
the final audit and spec §3's tie-break order, an exact tie on every term being a shared win (X-08).
Beside the demo: the P1 report and the headless balance check's report, from which the owner tunes
(item 127 (3)).

### 1.2 The machine-checkable half — AGENTS.md §10, item by item

| §10 item | What closes it at S1 | Task |
|---|---|---|
| 1. `cargo xtask ci` green on the matrix, the OSes agreeing on the chains | Every lane green on the three-OS matrix; the cross-OS guard over every chain, digest and path hash, including the new mapgen features golden | all; `demo` |
| 2. Hashes committed, every movement explained | The re-bless ledger of section 4.2; each chain-moving PR names the rule and the first tick that moved | `con1`, `fixs`, `tgtv`, `tgt`, `tgtw`, `mine`, `build`, `oper`, `tune` |
| 3. Golden playbooks round-trip, verify and render identically | The rewritten templates under `library/` and the new arms in `tests/golden/plan-core/` and `tests/golden/verifier/` | `tgtv`, `proj`, `oper` |
| 4. Headless scenarios assert on events **and** hashes | Five new scenarios under `scenarios/s1/` (`round-limit-audit` and `unaffordable-deploy` in `fixs`, `cover-nearest-vent` in `tgt`, `mine-to-depth` in `mine`, `brownout-by-recycle` in `grid`), beside the skeleton's four | `con1` (format), `fixs`, `tgt`, `mine`, `grid` |
| 5. Nightly adversarial scenarios | **Not this stage** — from S2 (AGENTS.md §10 item 5; `NIGHTLY_SCENARIOS_ENABLED` stays unset) | — |
| 6. Depth task and gate, or the written fallback | P1: QUICK p99 and the per-rule cost per decision tick, certified by a recorded run (section 5), or the spec's fallback taken in writing | `p1`, `tgt` (the counter), `demo` |
| 7. Docs in the same PR | Spec amendments (targeting's list; item 127 (5), (6), (7)'s "priority … orders brownouts and nothing else" in §5 and §7, (8)); AGENTS.md §3's crate-map row for `pharmakos-bench`, §3 rule 1's research-guard count, §3 rule 2, §3 rule 4's `rig.rs` keep-alive sentence (decision 17), §4.9, §5, §9's local-package sentence (decision 9) and item 11 — the main session's docs PRs, since lanes never edit AGENTS.md or `docs/` (the `pharmakos-bench` sentences land in the docs PR that lands with `p1`); `rules/README.md` is `con1`'s | main session; `con1` |
| 8. The owner reviews the demo | §1.1, from `docs/demo/s1-run-sheet.md` | `demo` |

### 1.3 What "ends playable" means here

Every item of the row, of §2.7's additions above, and of the register's S1 rows and carry-forwards
(section 2), including X-08's audit, the restore bug, S1-26 and S1-42, and nothing else. Where the
spec is silent the plan's readings are owner decisions (sections 6 and 7), none built before it is
ruled.

---

## 2. What §2.7 settled for S1, and where each ruling lands

| Ruling | What it says | Task |
|---|---|---|
| 126 (2) (f) | X-12: charge the placed beacon at deploy start, with F4; re-bless and explain; D-19's `$` 60 goes live | `fixs` |
| 126 (2) (h), (3) | F1–F4 into the first lane, F1 first; F5 and F6 with them. The first lane takes `fixc` (F1, F3, F6) and `fixs` (F5; F2 reproduced and F4 diagnosed), and X-12 itself. This departs from 126 (2) (h): F2's cure is the Mine rule (`mine`, wave 3) and F4's is targeting's per item 127 (12) (`tgt`, wave 2). Carried in the owner's summary for overrule | `fixc`, `fixs` |
| 126 (2) (e) | P2's baseline to S5, P6's to S3's playtest | not built |
| 124 (5) (g) | `editor.gd` releases GUI focus when it takes a ground or beacon click | `fixc` |
| 127 (2) | First Lull 10 min, later 5; a first-Lull `rules.proto` row; `lull_ms` 300 000 | `con1`; consumers `econ`, `ui` |
| 127 (3) | Economy rows proposed in range with owner PLACEHOLDERs; ~20 headless matches vs Easy | `check`, `tune` |
| 127 (5) S1-35 | The commander walks through a blackout, draws nothing; the rules text names the exception | `grid` |
| 127 (6) S1-36 | A unit's kind is its job; spec section 6's Common row reworded | `grid` |
| 127 (7) S1-22 | A priority raise re-applies the brownout order and relights at once when covered | `grid`, `oper` |
| 127 (8) S1-33 | The brownout skips a beacon whose shed relieves nothing | `grid` |
| 127 (9) S1-38 | All four Mine settings in S1; the verifier warns on a setting that needs S2 (decision 6 reads pillar spacing as needing no collapse, so the warning falls on SAFEST) | `mine`, `tgtv` |
| 127 (10) S1-40 | Settings screen and Probation's untimed Lulls to S6; lobby keeps three rounds | not built |
| 127 (12) | The targeting slice exactly as `targeting.md` "The S1 slice" lists it, including per-seat table room = the world total ÷ the seat count (amending item 63) | `con2`, `tgt`, `tgtw`, `tgtv`, `ui`, `oper` |
| 127 (13) | Per-seat `b_NN` and table room, per-viewer `e_NN`, save and snapshot bump — with targeting's behaviour PR; the seed withheld from seat tokens at **S3**, not S1 | `tgt` |
| 123 (4), 124 (5) (c) (e), 125 (3) (b) (c) | S1's first contract PR; 125 (3) (b) waits for the owner's live check of `claude.yml`, (c) is recorded unchanged | `con1` |
| 124 (5) (l) | The wizard's live `instantiate_template{suggested: true}` answer gets a report and prose golden (a gamectl lane) | `oper` |
| 126 (5) | M-05 with S1-41; `wave-lanes.js` pointed at this plan before wave 1 | `con1`; section 4.4 |

**The targeting slice, lane by lane** (targeting.md "The S1 slice", every line placed):

| Slice line | Task |
|---|---|
| Feature table and its mapgen check; `vent_`/`seam_` names | `tgt` |
| Per-seat `b_NN`, per-viewer `e_NN`, per-seat table room | `tgt` |
| `FeatureRef` with `VentPick`, `SeamPick` (`NEAREST`; `ANY`/`UNCOVERED`), `covered {}`; `Location.on`, `.covering` | `con2` (proto), `tgt` (behaviour) |
| Three reading rules and the failure table (`feature_lost` 11; `not_own` 10 retired) | `tgt` |
| "Nearest", `covering`'s spiral, the `on` rules; one structure per voxel | `tgt` |
| Walk-in, restart-keeps-beacon, no stacking | `tgt` |
| `Scope`'s features and the commander's position | `tgtw` (gateway's `verifier_scope`), `tgtv` (verifier) |
| `resolve_refs` (internal, `ADVISOR_METHODS`) and `get_map_summary.features` | `tgtw` |
| `estimate_route` accepting `covering`, and its selector bug fixed | `tgtw`; the bug itself in `fixs` |
| The three lints | `tgtv` |
| The chip; the editor's click and Alt-click | `ui` |
| Rewritten templates, Easy's names | `oper` |
| The Lull and recap messages | `tgtw` (text), `econ` (recap lines), `ui` (display) |

Targeting's size: the lanes above carry about **18 agent-days** of it (`con2` 2.5 of its 3, `tgt` 9,
`tgtw` 3, `tgtv` 1.5 of its 2.5, and about 2 inside `ui` and `oper`), **above** targeting.md's
14–16, which asked this plan to confirm the figure. It does not: the draft's own numbers already
summed to 17, and the reviews added `con2`'s refusals in the dependants and `tgt`'s re-sited harness
deploy. The owner's summary says so.

**Register rows this plan places outside a lane's own list:**

| Row | Where it lands | Task |
|---|---|---|
| S1-26 | The determinism harness's real segment, per decision 16; its anchors (`lib.rs`, `world.rs`'s `fill_unit_table`, `WorldConfig::units_per_seat`, `draw_new_destinations`, `tests/determinism.rs`, `DETERMINISM_TICKS`) are `tune`'s named places | `tune` |
| S1-34 | Every copy (`programs.rs`, `watch.rs`, `host.rs`, `viewfeed.rs`) reworded per decision 13 | `fixs` |
| S1-43 | Each lane that uncomments a `[workspace.dependencies]` version checks it against the registry; no S1 task | every lane |
| X-06 | `knowledge.rs`'s module doc reworded to what T14 built; kill credit named S2, radio S4 | `fixs` |
| U-03 | Deferred to hardening: no casual match is reachable in S1 (`serve.rs` always fogs, and the settings screen is S6's), so the defect cannot show; `fixs` marks it in `fog.rs` | `fixs` (marker) |
| M-14 | Not in S1 (decision 11 says why) | — |

---

## 3. The tasks

Conventions for every task, as in the skeleton: its own worktree (`git worktree add
../pharmakos-<id> -b <branch> main`), target directory `D:/build/<id>`, **one crate one agent**
(AGENTS.md §6), rebase on `main` before the PR, conventional commits, `git commit -s` with the
co-author line above it, SPDX headers, `cargo xtask ci` green before the PR opens, and every guessed
value a `// PLACEHOLDER: <what, who, when>` listed in the PR body. A lane opens its PR and stops;
the main session merges (item 126 (2) (g)). "Contract PR: yes" means an AGENTS.md §5 path; under the
delegation the main session merges it after the two reviews, and the PR body names the paths.
Estimates are agent-days (ad) including iteration to green, not review latency.

**Standing rules carried from the skeleton.** `xtask` and `.github/workflows/**` belong to no wave
agent except the task named for them, and at most one such PR is open at a time (`con1`, then `p1`,
then `tune`). At most one `proto/**` PR is open at a time (`con1`, then `con2`, then `tune` only if
a row retires). Each `→` in section 4.1 is a separate `wave-lanes.js` run, since a run launches all
its lanes at once. Each task's Implements line is its lane's `items`. Every task over 10 ad carries
a split seam written before it starts. Each lane rewords the register's stale and "ruled at the
demo" comments (D-20 to D-27, X-07, X-09 to X-11, items 123 (5) and 124 (5)) in the files it owns. A lane that adds a
committed scenario also owns its path constant and its `SCENARIOS` entry in
`crates/gamectl/tests/scenarios.rs`, the producer the `golden` step reads (item 129); the task
sections that add one name it in their Owns lines.

---

### `fixs` — `crates/sim` + `crates/gateway`: the demo's core findings, X-12 and the latent bugs

- **Owns:** `crates/sim` (except `compile_place` in `crates/sim/src/interpreter.rs`, `con2`'s named
  place, which `fixs` does not edit), `crates/gateway`, and named places in
  `crates/plan-core/src/strings.rs` (`AUTHOR_BUILTIN`, F5) and in `crates/plan-core/src/render.rs`'s
  test `the_envelope_and_the_blocks_are_rendered` (~769, which asserts the old wording), with the
  four `tests/golden/gateway/demo_*_render` goldens; `tests/golden/determinism`; the new
  `scenarios/s1/round-limit-audit` and `scenarios/s1/unaffordable-deploy`;
  `scenarios/skeleton/*.scenario.jsonc` (deadlines and notes only: `against-easy.scenario.jsonc`'s
  `by_tick` 1550 and its note's tick 1276, and their kin, move with the charge); a named place in
  `crates/gamectl/tests/operator.rs` (it asserts the Generator's kW); every golden its changes move.
  **Branch** `fix/sim-demo-findings`, worktree `../pharmakos-fixs`, target `D:/build/fixs`.
- **Builds:**
  - **X-12.** `run_deploy` (`crates/sim/src/interpreter/exec.rs`) charges
    `structures.beacon.cost_dollars` when the deploy starts (spec §7: "`$` leaves the treasury the
    moment an order commits: deploy starts"); an aborted deploy (death, leaving, an illegal site)
    refunds it in full (§5 Placement); a deploy the treasury cannot cover fails the step with the id
    decision 7 names. `World::place_beacon`'s stale PLACEHOLDER goes. The determinism harness's
    `deploy` step (`determinism_playbook`, `crates/sim/src/lib.rs`) now pays `$` 60, so the
    determinism chain moves, as item 126 (2) (f) said every chain would.
  - **F4's diagnosis** in the PR body: the repeat is a fixed voxel re-placed every round; X-12 now
    charges it; the cure (descriptions, no stacking) is `tgt`'s, per item 127 (12).
  - **F2 reproduced and explained.** The code map's cause: only a seam's exposed rim is minable
    (`dig_stand`, `crates/sim/src/world.rs`), so the start seam yields 24 voxels ($ 64 + $ 32) and
    then nothing. A regression test is committed red-and-ignored with the reason naming `mine`.
  - **The latent restore bug.** `World::restore_tables` (`world.rs` ~3801) sets `unit_limit` from
    the *restored* unit count, against `World::unit_limit`'s own doc (derived from the starting
    count); a match saved after fabricating resumes with more room than the unbroken run. Failing
    test first, then derive it from the starting count as construction does.
  - **X-08, the final audit.** A round-limit or no-survivor end names its winner by the final audit
    (spec §3: `$` held plus own assets at build cost × HP, plus the build cost of enemy assets
    destroyed) and spec §3's tie-break order (also item 16): unsmoothed net worth at the final
    audit, then enemy value destroyed, then fewer beacons lost, then a shared win — "there is no
    draw state", and only an exact tie on every term is a shared win. Held value exists
    (`World::held_value`); the destroyed term is always zero before S2's combat and kill-credit
    split, so it is read as zero behind a PLACEHOLDER naming S2 (the code map found it tracked
    nowhere). "Beacons lost" is a per-seat count read from the beacon table, which keeps a dead
    beacon's row for the whole match at zero HP — a recycled one included, since recycling books a
    death exactly as destruction does (item 18) — so it is already hashed, snapshotted state and
    adds no column; a test pins that a lost beacon's row survives a save and restore. **No new
    state:** `MatchOutcome.winner` (`crates/sim/src/runner.rs`) stays `Option<SeatId>`, `None` on a
    tie, and its hashed byte (`runner.rs` ~383–395) and the snapshot's `match_winner` keep their
    shape (`SNAPSHOT_VERSION` stays 6). The tied set comes from one sim function,
    `final_audit(&World)`, over already-hashed state (the treasury, held value, beacons lost); the
    recap and the gateway call it, and the recap's prose names every winner.
  - **Targeting's two named bugs.** `estimate_route`'s `place_of`
    (`crates/gateway/src/surface/knowledge.rs`) sends every ranked selector leg (`nearest`,
    `weakest`, `most_threatened`) to the seat's first beacon; it resolves the selector over the
    frozen snapshot as the sim's `resolve_beacon` does, through one snapshot-level resolver the sim
    exports (`resolve_beacon` is `pub(crate)` over `View` today, `cond.rs`), which `tgtw`'s
    `resolve_refs` reuses. `crates/sim/src/interpreter/cond.rs`'s module doc ("Selectors resolve
    once, at step start") is corrected: a condition resolves them at every evaluation.
  - **F5 in the gateway.** `PlanSealed` uses `count()` ("1 route step"); `fallback_engaged` names
    the posture (Hold, Shadow, Patrol) instead of "posture code 2"; a gateway-filed seal gets its
    own line in that seat's feed (decision 8's text); `AUTHOR_BUILTIN` reworded per decision 8.
  - **Two scenarios:** `scenarios/s1/round-limit-audit` (a round-limit end, its winner and the audit
    event) and `scenarios/s1/unaffordable-deploy` (the step fails with decision 7's id and `on_fail`
    takes it), each asserting events and hashes.
  - Comment rewording: X-06 (`knowledge.rs`'s module doc), X-07, X-09 (settlement is in-tick, not
    `end_recap`), X-10, X-11, D-20, D-25 to D-27, S1-34's four copies per decision 13, item 123
    (5)'s stale "Filled by T13" in these crates, item 124 (5) (k) and (m)'s sim half; U-03 gets a
    marker in `crates/gateway/src/fog.rs` naming hardening.
- **Does not:** no stacking, descriptions or walk-in (`tgt`); the Mine rule (`mine`); any proto or
  rules change; the first-Lull consumers (`econ`); `compile_place` (`con2`).
- **Implements:** items 16, 123 (5), 124 (5) (f) (k) (m), 126 (2) (f), (h), (3), 127 (12), 128;
  register X-06, X-08, X-12, S1-34, U-03; spec §3, §5 Placement, §7 Spending.
- **Needs:** decisions 7, 8 and 13 answered. Nothing merged to start; it opens its PR when green and
  **merges after** `con1`, the merge train rebasing it (it is `MATRIX_ONLY`, so it re-runs the
  matrix). The four `demo_*_render` goldens take `con1`'s line 8 (`phase_remaining_ms`) and this
  lane's line 5 (the prose), which are not adjacent.
- **Acceptance:** `a_deploy_charges_the_beacon_when_it_starts`;
  `an_aborted_deploy_refunds_the_beacon_in_full`;
  `a_deploy_the_treasury_cannot_cover_fails_its_step` (with `on_fail` taking it);
  `a_restored_match_keeps_the_unbroken_runs_unit_limit` (red first);
  `a_round_limit_end_names_the_final_audits_winner`; `a_net_worth_tie_goes_to_fewer_beacons_lost`;
  `an_exact_tie_on_every_term_is_a_shared_win`; `a_lost_beacons_row_survives_save_and_restore`;
  `a_starting_seam_yields_more_than_its_exposed_rim` (`#[ignore = "red until the mine lane
  (S1-38)"]`); `estimate_route_sends_a_nearest_leg_to_the_nearest_beacon`;
  `a_one_step_seal_reads_one_route_step`; `fallback_engaged_names_its_posture`;
  `a_timeout_filing_is_named_in_the_seats_own_feed`; the `tests/golden/interpreter/place_beacon`
  transcript re-blessed if it moves, with the reason (it is an event log and no event carries a
  charge, so `a_deploy_charges_the_beacon_when_it_starts` covers the charge); the two new scenarios;
  the scenario chains that place a beacon (all four of the skeleton's: `deploy-and-visit`,
  `against-easy`, `against-easy-three-rounds`, `expand-east-segment` and its event log) re-blessed
  with the first tick each diverges at; `tests/golden/determinism` re-blessed, the first diverging
  tick named (the harness's deploy step now pays `$` 60).
- **Contract PR:** no (item 124 (5) (f); X-08 adds no state, only a function over hashed state); the
  chains move and the body says so per chain, including any chain whose round-limit end now hashes
  the audit's winner in the runner's winner byte.
- **PLACEHOLDERs:** the audit's destroyed-value term (owner, S2, with the kill-credit split).
- **Agent-days:** 5.5–6.5.

### `fixc` — `crates/client-gdext` + `godot/`: F1 first, then F3 and F6

- **Owns:** `crates/client-gdext`, `godot/`, their goldens (`tests/golden/vista/*.png` re-rendered
  on CI's Linux leg). **Branch** `fix/client-demo-findings`, worktree `../pharmakos-fixc`, target
  `D:/build/fixc`.
- **Builds:**
  - **F1 first.** Reproduce by hand with the window focused, logging `button_down` and `pressed` on
    Submit, Fix and Save and `_input` in the lobby. The code map's candidates, most likely first: an
    open map `PopupMenu` (`_on_ground_clicked`, `_on_beacon_clicked` in `godot/scripts/editor.gd`)
    swallows the next click; `Rows.fill` frees the Fix buttons under the cursor on every `changes`
    tick; the ScrollContainer's state after a wheel scroll. Fixes: close the map menus when the
    panel takes a click; `get_viewport().gui_release_focus()` in `_unhandled_input` (item 124 (5)
    (g)); rebuild the rows only when they or their enabled state change; `FOCUS_NONE` on Fix;
    "Submitting…" and "Fixing…" status. "Save with no file" is expected to be F1 again; verify it,
    and if it is not F1, Save with no file opens Save as….
  - **F3.** Reproduce with the code map's four steps (B with `--verbose --log-file`; B with its own
    `APPDATA`/`LOCALAPPDATA`; `--rendering-driver opengl3`). If a shared `user://` is the cause, a
    single-instance guard: the second instance says "Pharmakos is already running." and quits;
    `application/run/flush_stdout_on_print` so the next hang leaves a log. If it is not, stop and
    report (decision 19). *Item 129 (2) (c): F3 did not reproduce, the flushed log landed, and the
    guard is deferred until F3 recurs in the next round of testing.*
  - **F6.** The speed label only in a Push (`lobby.gd`); the chooser hides the speed strip and the
    editor panel until a match starts; the smoke check's two `ERROR` lines traced to their source
    and an expected refusal demoted from `godot_error!`. Camera wheel zoom: the owner's, by hand.
  - Item 124 (5) (n) and S1-45: one shared stand-in for `tests/rate_budget.rs` and
    `phase_budget.rs`. D-21 to D-24 comments reworded as ruled. `godot/scripts/watch_check.gd`'s
    header comment (~17, "round 1's 180-second Lull timer") names no length, since `con1` makes it
    300 s and `ui` 600 s.
- **Does not:** the first-Lull timer, the chip, recap lines (`ui`); `no_arithmetic.rs`'s widening
  (S1-44, `ui`); `xtask/src/package.rs`.
- **Implements:** items 124 (5) (g) (n), 126 (2) (h), (3); register S1-45, D-21 to D-24.
- **Needs:** nothing merged to start; it opens its PR when green and **merges after** `con1` (which
  edits `mesher_rules.gd` and two `godot/fixtures/` copies), the merge train rebasing it (it is
  `MATRIX_ONLY`, so it re-runs the matrix); `con2` merges after it. It leaves the two `Status`
  literals `con2` names in `crates/client-gdext` alone. F3's fix shape may need decision 19.
- **Acceptance:** `watch_check.gd` gains `_first_click_lands`: after a rows redraw, with a map menu
  open and after a panel scroll, one injected click on Submit, Fix and Save reaches its handler
  exactly once; `godot_project.rs` pins the focus release, the hidden chooser controls and the guard
  text (the guard text waived by item 129 (2) (c)); the PR body records F3's reproduction and cause; the main session's local `cargo xtask
  package` smoke log holds no `ERROR` line (item 124 (5) (c): lanes do not run it).
- **Contract PR:** no. **PLACEHOLDERs:** none expected. **Agent-days:** 3–4.

### `con1` — S1's first contract PR: rules rows, `xtask`, toolchain, REUSE, `.github`

- **Owns:** `proto/gp/v1/rules.proto` and the generated tree, `proto/buf.yaml`, `rules/`, `xtask/`,
  `.github/workflows/`, `rust-toolchain.toml`, `.cargo/config.toml`, `Cargo.toml`
  `[workspace.package]`, `clippy.toml`, `REUSE.toml`, `.gitignore`,
  `proto/gp/api/v1/gateway.proto`'s comments only; the whole of `crates/proto` (its tests pin the
  rules table: `the_rules_table_is_in_canonical_form` at `tests/proto.rs` ~733,
  `the_restated_rows_agree` at ~1024, and the item-90 pins); `tests/golden/{proto,docs,verifier}`
  (`expected.rules.v1.json`; `expected.reserved.txt`, whose `RulesTable.Economy` goes 12–15 → 13–15;
  the rules table's lines of `tests/golden/docs/reference/expected.docs.txt`, ~342–435; every
  verifier `report_hash` golden); and named places in `crates/gamectl/src/scenario*` (the scenario
  format), `crates/gamectl/src/doctor.rs`, `crates/gamectl/tests/parity.rs` (its Lull reading),
  `crates/gateway/tests/` (the two `LULL_MS` constants, `tests/support/mod.rs` ~50 and
  `tests/methods.rs` ~72, and the goldens' kin), `godot/scripts/mesher_rules.gd` (`RULES_JSON`'s
  `match` member, which `crates/client-gdext/tests/godot_project.rs` holds equal to
  `rules.v1.json`), `godot/fixtures/instantiate_suggested.json` (~35, `phase_remaining_ms`) and
  `godot/fixtures/view_keyframe.jsonl` (its one line), which are byte copies of the gateway goldens
  that `crates/client-gdext/tests/wizard.rs` (~53–90) and `vista_fixture.rs` (~69–78) hold equal,
  `crates/mesher/src/upload.rs` (a comment), and the goldens those move:
  `tests/golden/gateway/{demo_*,instantiate_suggested,walkthrough*,view_keyframe}` wherever they
  carry the Lull's length. **Branch** `build/s1-first-contract`, worktree `../pharmakos-con1`,
  target `D:/build/con1`.
- **Builds:**
  - **Rules rows (additive):** `match.first_lull_ms` = 600 000 and `match.lull_ms` 180 000 → 300 000
    (item 127 (2), D-01); `structures.build_hp_per_second` = 60 (S1-24);
    `economy.mining_carry_voxels` = 16 (named so by item 129 (2) (a): the gateway's confinement
    test refuses `load` in a `gp.*` field) in `Economy`'s reserved 12 (S1-25), which narrows
    `reserved 12
    to 15` to 13–15 and so trips buf's `RESERVED_MESSAGE_NO_DELETE`: `proto/buf.yaml` gains the
    scoped `ignore_only` for `gp/v1/rules.proto` its own comment prescribes, with the justification,
    and `con2` replaces it with its own. **No spacing row:** the minimum beacon spacing targeting.md
    asks for is ruled "no stacking only" (decision 10), which is a legality rule in the sim (`tgt`),
    not a row. Each new row with an owner PLACEHOLDER at S1's demo; the table's `revision` goes 3 →
    4 (`rules.proto`: bumped whenever a value changes); `rules/README.md`'s Settled? table updated.
    `commander.placement_range_voxels` documented as superseded by walk-in (item 127 (12), amending
    item 11).
  - **Item 123 (4):** S1-01 (pin 1.98.1; `rust-version` and `msrv` to 1.98), S1-02 (the G4 wait
    reworded to G4's recorded result), S1-03, S1-04, S1-05 to S1-07 and S1-08 as decision 9 rules,
    S1-10's grammar and its `cargo xtask placeholders` command, M-03.
  - **Item 124 (5) (c) and (e)** as decision 9 rules. **Item 125 (3):** (b) waits for the owner's
    live check of `claude.yml` (handoff NEXT 3) and (c) is recorded, unchanged, so neither is built
    here (decision 9).
  - **X-14** ratified; **X-15, X-17, X-18** name their stage in the proto comments (decision 9).
  - **S1-41, M-05, X-01:** the scenario format gains `units_per_seat`, `round_limit` and a
    `length_ms` cap; M-05 as decision 9 rules.
  - Item 124 (5) (i) (`.gitignore`'s cache pattern) and (m)'s `upload.rs` and `rules/README.md`
    halves.
- **Does not:** read the new rows in the sim (the sim lanes swap their constants); the per-round
  Lull of the gateway and `gamectl` (`econ`) and of the client (`ui`); any targeting proto (`con2`).
- **Implements:** items 123 (4), 124 (5) (c) (e) (i) (m), 125 (3) (b) (c) (recorded, not built), 126
  (5), 127 (2), (12), 128; register S1-01 to S1-10, S1-24, S1-25, S1-41, M-03, M-05, X-01, X-14,
  X-15, X-17, X-18.
- **Needs:** decisions 9, 10 and 13. `fixs` and `fixc` merge after it, the merge train rebasing
  them.
- **Acceptance:** `buf lint`; `buf breaking` in `WIRE_JSON` mode against `main` with the scoped
  ignore; `expected.reserved.txt` shows `RulesTable.Economy` 13–15;
  `the_rules_table_is_in_canonical_form`; `the_restated_rows_agree`; `wizard.rs` and
  `vista_fixture.rs` green over the re-copied `godot/fixtures/`; `godot_project.rs` green with
  `mesher_rules.gd`'s `lullMs` at 300 000; `xtask` unit tests for `PHARMAKOS_REQUIRE_TOOLS` parsing,
  the three fall-backs failing, and every child going through `child_command`; every `report_hash`
  golden re-blessed with the reason "`rules_hash` moved: three rows added, `lull_ms` 300 000,
  `revision` 4"; the ten `demo_*` goldens moved (`phase_remaining_ms` 300 000) and explained; the
  main session's local `cargo xtask package` run, since `con1` changes `rules/` and `REUSE.toml`;
  **no chain moves** (the sim does not read `lull_ms`, and the new rows hold today's constants) — if
  one does, the lane stops and says why.
- **Contract PR:** yes (`proto/**`, `buf.yaml`, `xtask`'s `ci`, workflows, REUSE, toolchain, the
  scenario format).
- **PLACEHOLDERs:** the three new rows' values (owner, S1's demo). **Agent-days:** 3–4.

### `con2` — the targeting proto and the S1 wire (targeting contract PR 1)

- **Owns:** `proto/gp/v1/playbook.proto`, `proto/gp/api/v1/gateway.proto`, `proto/buf.yaml`, the
  whole of `crates/proto` (the generated tree and its tests, after `con1`, which owns it first),
  `tests/golden/{schema,docs,proto}`, the new test files `crates/sim/tests/targeting_refused.rs` and
  `crates/verifier/tests/targeting_refused.rs` (inline asserts, no golden), and named places for the
  exhaustive matches a new `Location` arm breaks: `compile_place` in `crates/sim/src/interpreter.rs`
  (which `fixs`, the sim's owner, does not edit), `check_location` in
  `crates/verifier/src/structure.rs`, `walk_location` in `crates/verifier/src/walk.rs`, and `fn
  location` in `crates/plan-core/src/render.rs` (~318); and, for the new `Status` field, the two
  whole-struct `Status` literals in `crates/client-gdext` (`round_trip_status` in `src/bridge.rs`
  ~1528, and `tests/fixtures.rs` ~69; `rig.rs` ~1516 uses `..Default::default()` and does not
  break). **Branch** `feat/proto-targeting`, worktree `../pharmakos-con2`, target `D:/build/con2`.
- **Builds:** `FeatureRef { feature_id = 1; VentPick vent = 2; SeamPick seam = 3; Covered covered =
  4 }`, `VentPick`/`SeamPick` with `rank` (`NEAREST`) and `coverage` (`ANY`, `UNCOVERED`), each
  `*_UNSPECIFIED = 0`; `Location.on = 10`, `Location.covering = 11`, reserved narrowed to 12–49;
  every proto comment targeting.md's contract PR 1 lists; `BuildSettings` 6 and 7's comment and
  `MineSettings`' PLACEHOLDER per decisions 5, 6 and 11; `gateway.proto`: `resolve_refs` request and
  response, `GetMapSummaryResponse.features` (S1-48, from its `reserved 5 to 15`),
  `GetRecapResponse`'s settlement lines (X-16, from its `reserved 2 to 15`), the forecast fields
  decision 12 keeps (S1-46, from `GetEconomyForecastResponse`'s `reserved 5 to 15`), S1-11's "no
  countdown" as a new `Status` field 5 (decision 12; `econ` fills it), M-11's row catalogue as a
  comment (decision 11). X-03 needs no field: `GetBriefingResponse.standing` already carries rank
  and score, and the sentence is `econ`'s. **The scoped `buf.yaml` ignore replaces `con1`'s** and
  covers `gp/v1/playbook.proto` and `gp/api/v1/gateway.proto`, each discharge named in its
  justification. **The new arms are refused until their lanes land:** the sim's compile returns
  `PlanError::NotAtThisStage` naming the construct, as it does for today's held constructs
  (`crates/sim/tests/interpreter.rs` ~544–589; `tgt` replaces it), the verifier emits E0003 for them
  (`tgtv` replaces it), and `render_plan` names them plainly.
- **Does not:** any behaviour beyond those refusals; any other sim change; fill the `Status` field
  (`econ`).
- **Implements:** item 127 (9), (12) (targeting.md's contract PR 1), 128; register S1-11's wire,
  S1-46, S1-48, X-16, M-11.
- **Needs:** `con1` merged; it merges after `fixc` too (the `Status` literals); decisions 3, 4, 5,
  6, 11 and 12.
- **Acceptance:** `buf lint`; `buf breaking` green with the scoped ignore; the reserved-numbers
  golden moving for exactly `Location` (12–49), `GetRecapResponse`, `GetMapSummaryResponse` and
  `GetEconomyForecastResponse`, each named in the PR body; canonical JSON round-trips for every new
  message; `targeting_refused.rs` in the sim (a playbook using `covering` or `on` fails to compile
  with `PlanError::NotAtThisStage` naming the construct) and in the verifier (it verifies with
  E0003); `get_schema` and `gamectl docs` goldens re-generated; no chain and no `report_hash` moves.
- **Contract PR:** yes. **PLACEHOLDERs:** none. **Agent-days:** 2.5–3.5.

### `tgt` — `crates/sim` + the gateway's ids: targeting's determinism and behaviour (PRs 2 and 3)

- **Owns:** `crates/sim` (the determinism harness in `lib.rs` and `tests/allocations.rs` included);
  in `crates/gateway` only what the id change forces (the view, `list_beacons`, events, the save and
  `match.json` formats, `GATEWAY_VERSION`); `crates/gamectl/tests` that name ids, and any scenario's `SCENARIOS` entry it adds (item 129); a named place in
  `crates/operator`'s tests that name ids, with `tests/golden/operator`; the re-bless of
  `tests/golden/verifier` and `tests/golden/gateway/*_verify`, with
  `godot/fixtures/rows_report.json` (a named place, re-copied only if the four verifier goldens it
  holds move); every golden that moves. **Branch** `feat/sim-targeting`, worktree
  `../pharmakos-tgt`, target `D:/build/tgt`.
- **Builds:** targeting.md's contract PRs 2 and 3 in one PR, re-blessed once: per-seat `b_NN` (a
  seat's core is `b_00`) and per-seat table room (world total ÷ seat count, rounded down, as item
  127 (12) ruled), per-viewer `e_NN`; the feature table in mapgen with its duplicate-anchor and
  overlap check (`MapError`), regenerated on restore and not hashed per tick, `power.rs`'s
  `one_vent` moved onto it (discharges S1-32); derived liveness by footprint scan; the resolver and
  "nearest" (the item-61 estimator's `cost`, unreachable skipped, ties by anchor y then x); the
  three reading rules; `covering`'s spiral and the `on` rules; `covered {}`; one structure per voxel
  across seats; walk-in with `timeout_ms` bounding the walk and `no_path`; restart keeps the placed
  beacon; no stacking, a legality rule with no row (decision 10); `feature_lost` = 11, `not_own`
  retired; the new hashed state (bindings in `PlanState`, each Build target's description and bound
  feature id, the beacon ordinal, the restarted step's beacon and rows) in the hash, the snapshot
  **and** the goldens; `SNAPSHOT_VERSION`, the save version and `GATEWAY_VERSION` bumped, refusing
  old saves with a clear message; `fixs`'s charge moved to where the deploy starts after the
  walk-in, never at step start; `con2`'s refusals in `compile_place` replaced by the behaviour, and
  `PlanError::NotAtThisStage`'s doc table brought in line with what still refuses (item 130).
  **The harness deploy re-sited:** `determinism_playbook` deploys at the `safest` beacon's own
  position, which no stacking makes `illegal_site`, so it moves to a legal site (determinism code)
  and `tests/allocations.rs` still sees the deploy land. **P1's counter:** an unhashed
  evaluation-units count per decision tick per seat (handlers tried, condition nodes, selector
  candidates, "nearest" estimates), reusing `seams::WorkCounter`, which `tests/determinism.rs`
  already keeps out of the state encoding.
- **Does not:** `resolve_refs`, `get_map_summary.features`, `estimate_route` with `covering`,
  `Scope`'s features (`tgtw`); the verifier's rules (`tgtv`); the operator's behaviour (`oper`);
  `toward` (decision 3).
- **Implements:** item 127 (12), (13); targeting.md; amends items 11 and 63; register S1-32, S1-37;
  P1's counted half (item 33 (c)).
- **Needs:** `con2` and `fixs` merged, and `tgtv` merged first (it changes `report_hash`'s layout,
  which this PR's re-bless then covers); decisions 3 and 4.
- **Acceptance:** `the_nearest_vent_is_the_least_travel_not_the_least_distance` (a vent across a
  ravine loses); `ties_go_to_anchor_y_then_x`; `a_held_target_fails_only_when_lost`;
  `a_carried_covering_step_has_no_legal_site_after_round_one_on_the_golden_seed`;
  `a_second_generator_on_a_covered_vent_is_illegal_site`; `stacking_on_an_own_beacon_is_illegal`;
  `a_restarted_step_resumes_at_the_beacon_it_placed`; `the_walk_in_times_out_with_no_path`;
  `a_seats_core_is_b_00_and_ids_are_per_seat`;
  `another_seats_beacon_is_e_nn_in_first_sighting_order`; `a_pre_s1_save_is_refused`;
  `the_feature_table_is_regenerated_identically_on_restore`; a new
  `tests/golden/mapgen/expected.features.txt` (a format change); the new scenario
  `scenarios/s1/cover-nearest-vent` asserting `beacon_placed`, round 2's `step_failed` (its
  `illegal_site`, by item 131 (4) (a), pinned by the chain and by the sim test above, since
  `event_fired` has no value predicate) and hashes; `the_decision_tick_counts_its_evaluation_units`
  (counted work, asserted, unhashed); `allocations.rs` green over the re-sited deploy; **every
  chain** (the determinism chain included) re-blessed once, the first diverging tick named per
  chain; every `report_hash` golden re-blessed (the snapshot version and the ids); every gateway,
  operator, interpreter and economy golden that names an id explained.
- **Contract PR:** yes (determinism code, the snapshot and save formats, a golden format).
- **PLACEHOLDERs:** the per-seat numbers (owner, S1's demo, with the check's report; the totals
  revisited at S2's G3′-real). **Agent-days:** 8.5–9.5. **Split seam:** ids, room and the save bump
  as commit set 1; the feature table, resolver, sites and the harness re-site as set 2 — still one
  PR, re-blessed once.

### `tgtw` — `crates/gateway`: targeting's surfaces

- **Owns:** `crates/gateway`. **Branch** `feat/gateway-targeting`, worktree `../pharmakos-tgtw`,
  target `D:/build/tgtw`.
- **Builds:** `resolve_refs` over the frozen world, on `ADVISOR_METHODS`, internal, through the
  snapshot-level resolver `fixs` exports; `estimate_route` accepting `covering` and returning the
  site (discharges S1-20); `get_map_summary.features` (id, kind, grade, live, coverage by the seat,
  travel, reachable; S1-48; `live` kept by item 130 (3) (a)); `verifier_scope` carrying the features
  (id, kind, grade, live bit) and the commander's position; `core_beacon_of` becomes `b_00` (S1-13);
  the Lull's "this round: …" message on re-seal and the recap's "why a step found nothing" text;
  carried by item 130, the stale `get_map_summary` doc in `surface/knowledge.rs` (~331–336: its
  `reserved 5 to 15` and its "what *is* fogged … vents, seams") and `surface.rs`'s "Four methods …
  no request/response pair", in the module doc and at the dispatch catch-all (~68–71, ~2429–2436),
  untrue of `resolve_refs` until this lane serves it. Carried by item 131: `verifier_scope` fills
  each feature's anchor column (`x`, `y`) and `covered_by` (the seat's own covering beacon, lowest
  id) as well as targeting.md's four, because two of `tgtv`'s three lints read them (item 131 (4)
  (d)).
- **Does not:** rank in the verifier; publish anything (v1.1).
- **Implements:** item 127 (12) (targeting.md's gateway lines); register S1-13, S1-20, S1-48.
- **Needs:** `tgt` merged. **Acceptance:** `resolve_refs_answers_as_the_sim_would_at_step_start`
  (the gateway's pick equals the sim's on the golden seed);
  `estimate_route_returns_the_covering_site`; `a_hidden_or_foreign_name_answers_no_target`; the
  confinement test unchanged; the method and walkthrough goldens; the gateway's `report_hash`
  goldens (`*_verify`, the walkthrough) re-blessed, the `Scope`'s content now filled.
- **Contract PR:** no (a `report_hash` input's content, not its format; `tgtv` changed the format).
  **Agent-days:** 2.5–3.

### `tgtv` → `proj` — `crates/verifier` + `crates/plan-core`, one lane in two PRs

- **Owns:** `crates/verifier`, `crates/plan-core`, and a named place in
  `godot/fixtures/rows_report.json`, which `crates/client-gdext/tests/editor_fixtures.rs` (~170–194)
  holds equal to four verifier goldens' `diagnostics`, re-copied when they move; for `proj`, a
  named place in `crates/bench/src/quick.rs` (`fixture_scope`, `map_scope`, `MAP_CASES`,
  `fixture_snapshot`), restated in the same PR if it changes the verifier goldens' fixture, which
  `crates/bench/tests/harness.rs` holds equal (item 131 (5)). **Branches**
  `feat/verifier-targeting` then `feat/verifier-estimate`, worktrees `../pharmakos-tgtv`,
  `../pharmakos-proj`, targets `D:/build/tgtv`, `D:/build/proj`.
- **`tgtv` builds:** `Scope`'s features and the commander's position; which arm is legal where
  (`covering` only in `PlaceBeaconStep.at`, `on` only in a `BuildTarget.anchor`, `covered {}` only
  under `on` inside a `covering` deploy's `initial`); `RANK_UNSPECIFIED` and, under `covering`,
  `COVERAGE_UNSPECIFIED` as errors; `coverage` under `on` refused; feature `NEAREST` in a condition
  refused; the three lints and their catalogue rows; the warning for `seam_choice: SAFEST` (reads as
  NEAREST until S2), a new catalogue code (item 127 (9), decision 6); `render_plan` for the new
  arms, replacing `con2`'s refusals; S1-39 per decision 11; item 124 (5) (h). Carried by item 130:
  the refusal replaced is `con2`'s `HeldSites` visitor in `structure.rs`, not a second one beside
  it, together with E0003's message and its catalogue row's emitter; `render_plan`'s two `con2`
  strings move into `strings.rs` with a render test; the S1-39 doc in `structure.rs` (~232–247)
  re-cited to decision 11. Never ranks. Merges before `tgt`, whose PR re-blesses every
  `report_hash` golden over the new `Scope` layout. 2–3 ad.
- **`proj` builds:** the estimate stage (S1-47): the projection arithmetic moved **down** into the
  verifier (plan-core re-exports it), breaking the plan-core → verifier cycle the code map found;
  units and Build targets in the projection with the charged beacon; E0601, W0601, W0602, W0603 and
  the route-fits-the-segment codes W0701 and I0001 emitted; W0603's embedded spaces fixed; S1-23,
  X-05 and U-04 per decision 15; M-10 (`Options` stays `allow_dormant_beacons` alone). Carried by
  item 131: the two other strings with embedded spaces in `crates/verifier/tests/verifier.rs`, and
  `tests/golden/verifier/README.md`'s "No producer exists" on `budget_128` (`crates/bench` is it),
  with its fixture table's `b_01` "the seat's pre-placed core". No stepping, no future evaluated:
  `$`/`kW` projection is Quartermaster arithmetic (AGENTS.md §3 rule 2). 5–6 ad.
- **Implements:** `tgtv`: item 127 (9), (12); register S1-39; item 124 (5) (h). `proj`: register
  S1-23, S1-47, X-05, U-04, M-10; P1's QUICK path (item 33 (c)).
- **Needs:** `tgtv`: `con2` merged; decisions 6 and 11. `proj`: `tgtv` and `fixs` merged (the
  charge); decision 15. `proj` merges after `tgt`, rebasing onto it and re-blessing
  `tests/golden/verifier` over `tgt`'s, so the two never re-bless it at once; that pass is half a
  day in wave 3's third slot, before `grid` (section 4.1).
- **Acceptance:** a case under `crates/verifier/tests/cases/` for every newly emitted code
  (`every_emitted_code_has_a_case`); `nothing_the_skeleton_emits_hangs_off_an_empty_full_stage`
  replaced by its S1 form; `full_finds_what_quick_finds`; `the_verifier_never_ranks` (a source-text
  test that `estimate` is not reached from `resolve`); report goldens' `diagnostics` and `qualifies`
  moved and explained (`tgtv`: every `report_hash` too, the `Scope`'s encoding having widened;
  `proj`: every `report_hash` if it moves the verifier version); the catalogue golden; no chain
  moves.
- **Contract PR:** `tgtv` yes (`report_hash`'s scope encoding); `proj` no. **Agent-days:** 7–9 in
  all.

### `p1` — the walled timing crate: P1's wall-clock half (the depth task)

- **Owns:** a new crate (recommended `crates/bench`, package `pharmakos-bench`, never shipped),
  `Cargo.toml`'s members, `xtask/src/main.rs` (`WALLED_PACKAGES`, `GUARDED_PACKAGES`, `perf_alarms`,
  and the doc comments of `CLIENT_WALL` and `WALL_GUARDED_PACKAGES`), `clippy.toml`'s header;
  and, carried by item 129, the client check's own user folders in `xtask/src/main.rs` (as
  `xtask/src/package.rs` already gives the smoke run) and a fresh `golden` output per run, so a
  warm target cannot compare a stale `actual.*`.
  Nothing in `crates/sim`: the counted half is `tgt`'s hunk. **Branch** `feat/bench-p1`, worktree
  `../pharmakos-p1`, target `D:/build/p1`.
- **Builds:** section 5's harness per decision 2: QUICK's timing, and the wall-clock cost of a
  decision tick per size unit from a hosted match whose playbooks sit at the size budget. The crate
  takes no `CLIENT_WALL` line (section 5), so this PR rewords the doc comments that would then be
  wrong in `xtask/src/main.rs`: `CLIENT_WALL`'s "a future walled crate joins by adding its own
  line", and `WALL_GUARDED_PACKAGES`' "[`GUARDED_PACKAGES`] plus `sim`", which stops being true once
  the walled `bench` is in the research guard. AGENTS.md's matching sentences (§3's crate map, §3
  rule 1's "any of the five", §4.9's "the research guard's five crates plus the sim" and its
  `CLIENT_WALL` sentence) are the main session's, in the docs PR that lands with this one (section
  4.4).
- **Implements:** item 33 (c), item 116 (6) (b); spec §16's P1 row (S1's half).
- **Needs:** decision 2; `con1` merged (one `xtask` PR at a time). The certifying run waits for
  `proj` and `tgtw` (both change verifier cost) and for `tgt`'s and `mine`'s counters; `demo` runs
  it.
- **Acceptance:** `wall-guard` green with the new crate walled and no deterministic crate reaching
  it; the research guard green with it in `GUARDED_PACKAGES`; `cargo xtask perf-alarms` prints QUICK
  p50/p99 over the committed cases and `budget_128`, and ns per size unit per decision tick, per
  runner, as `::notice::`s with no threshold.
- **Contract PR:** yes (§4.9, `xtask`). **Agent-days:** 4–5.

### `check` — the headless balance check (item 127 (3))

- **Owns:** a new `crates/gamectl/tests/balance.rs` (a named place; beside it in the gamectl tests
  runs only `fixs`'s named place in `operator.rs`). **Branch** `test/gamectl-balance`, worktree
  `../pharmakos-check`, target `D:/build/check`.
- **Builds:** an `#[ignore]`d test, run by `cargo test --release -p pharmakos-gamectl --test balance
  -- --ignored`, that plays 20 seeds × {Easy vs Easy, the Hold & Build template vs Easy} × {3
  rounds, 6 rounds} — 80 matches — through `serve::InProcessSeats` (the
  `crates/gamectl/tests/operator.rs` pattern) and writes a report under `CARGO_TARGET_TMPDIR`
  (`balance/report.txt`): per seat per round the treasury, income by source, supply, draw and
  structures; it flags a treasury that only grows, a seat that stalls (no new asset for two rounds),
  and a seat that never affords a Generator. **Not a `cargo xtask ci` step**, never gated, never
  compared across OSes. **Its run time:** on the 3 / 5 / 8 ladder (the later rounds 8 minutes) a
  3-round match is 19 200 ticks and a 6-round one 48 000, so the 80 matches are about 2.69 M ticks.
  Measured at `352e573` in release on the owner's machine, `against-easy-three-rounds` (6 000 ticks)
  runs in about 1.1 s: about 0.1 ms a tick after about 0.45 s of start-up. That puts the check at
  about 5 minutes at the skeleton's load, run one match after another; the plan budgets 15–25
  minutes for S1's heavier late rounds (more assets, mining and "nearest" estimates).
- **Implements:** item 127 (3).
- **Needs:** nothing merged (it drives the match through the gateway, not the scenario format).
  Re-run by `tune` on the finished rules.
- **Contract PR:** no. **Agent-days:** 1.5–2.

### `mine` — `crates/sim`: finite seams and all four Mine settings (S1-38, item 127 (9))

- **Owns:** `crates/sim` (except `grid`'s named place), and its scenario's path constant and
  `SCENARIOS` entry in `crates/gamectl/tests/scenarios.rs` (item 129). **Branch** `feat/sim-mine-settings`,
  worktree `../pharmakos-mine`, target `D:/build/mine`.
- **Builds:** the interpreter stores `MineSettings` (the empty `Mine(_)` arm); `dig_max_depth`,
  `pillar_spacing`, `seam_choice` and "never digs under structures" per decision 6, replacing
  `dig_stand`'s exposed-rim rule with a pit-safe one; the seam choice held until the seam is spent,
  using `tgt`'s "nearest"; a spent seam reads as spent; the Mine request filed under its own urgency
  band (spec §7's ladder: mine last) instead of `Urgency::Units`; `MINING_LOAD_VOXELS` read from its
  row, `economy.mining_carry_voxels`, with the two comments that still say `mining_load_voxels`
  (`crates/sim/src/economy.rs`, `crates/sim/tests/economy.rs`) reworded; S1-27 to S1-30 per decision 13; each "nearest" estimate the seam choice makes counted on
  `tgt`'s evaluation-units counter (P1). Carried by item 131: why the sim's `set_mandate` row
  keeps the kind alone (`Row::Mandate { kind }`) and drops the settings it carries, which the
  verifier's I0003 reads as starting from the defaults; fixed here, or put to the owner.
- **Implements:** item 127 (9); register S1-25's reader, S1-27 to S1-30, S1-38; spec §6's Mine row,
  §7's urgency ladder.
- **Needs:** `tgt` merged; decisions 6 and 13. **Acceptance:** `fixs`'s F2 test un-ignored and
  green; `digging_stops_at_the_max_depth`; `pillars_stand_at_their_spacing`;
  `no_voxel_under_a_structure_is_dug`; `a_held_seam_is_kept_until_spent`;
  `richest_means_richest_remaining`; the economy golden delivering in rounds 2 and 3; new scenario
  `scenarios/s1/mine-to-depth`; every scenario chain re-blessed (every seat mines), reasons named;
  `tests/golden/determinism` re-blessed (the harness's Mine beacon now digs past the rim), the first
  diverging tick named; every `report_hash` golden re-blessed if the held seam bumps the snapshot.
- **Contract PR:** yes in part (the held seam choice extends hashed state and the snapshot).
- **PLACEHOLDERs:** seam shape and clearance constants (owner, S4's map re-derivation) if decision
  13 leaves them. **Agent-days:** 5–6.

### `fog` — `crates/sim`: targeting's fog follow-up (item 133 (3) (c), (h), (j))

- **Owns:** `crates/sim` (except `grid`'s named place), and a named place in
  `crates/gateway/src/strings.rs`: `step_failed`'s line and its tests there, which read the
  event's value (item 134 (2) (a)); every golden its change moves. **Branch**
  `fix/sim-targeting-fog`, worktree `../pharmakos-fog`, target `D:/build/fog`.
- **Builds:**
  - **The sphere test first** (item 133 (3) (c)). In `targeting.rs`'s `on_vent`, a named or
    `covered {}` `on` first tests the vent's anchor point, its anchor column's standing point
    (`anchor_point`), against the target sphere (within `sphere_radius()` of the centre). No
    structure can move that point; `Ground::on_column` cannot be the test, because it picks a free
    column by reading structures. Outside the sphere the answer is `no_target`, whatever stands on
    the vent, so no answer depends on a structure the seat cannot see (`no_target`, so a name reads
    as a description does). Inside, the existing tests follow: a `taken` footprint or no free
    column is `illegal_site`, and so is a free `on` column that lies outside the sphere. Item 131
    (4)'s `illegal_site` reading narrows to vents inside the sphere. `Nearest` and `on_candidate`
    are unchanged, and so is `cover`: `fog` changes only `on` picks, so `cover-nearest-vent`'s round
    2 still fails `illegal_site`. The sphere test is published as a `pub fn` beside the three
    predicates below, for `econ`'s `single` (item 134 (2) (b)).
  - **The count on `step_failed`** (item 133 (3) (h)), so the recap can say "3 matched, none
    reachable". The count is the candidates `matches_pick` or `on_candidate` admitted before
    reachability was asked, the same meaning as `resolve_refs`'s `matched`: 1 or 0 for a name or
    `covered {}`, and 0 for one outside the sphere. An absent count is distinct from 0 (no magic
    value). The value slot's layout is written once in a public sim type with a typed decode, and
    a bare reason id decodes as the reason with no count, so the gateway's feed test
    (`crates/gateway/tests/targeting.rs` ~762–776), `strings.rs`'s `RETIRED_NOT_OWN` test and the
    event-log goldens pass unchanged. `cover`, `on_vent`, `Ranker::new` and the `StepFailure`
    variants stay source-compatible, because the gateway calls them (`surface/knowledge.rs` ~813,
    `src/targeting.rs` ~66, and `surface/planning.rs` ~676–683, which matches unit variants): the
    count travels through a new counted form or accessor. The gateway's `step_failed` line uses the
    typed decode, its words unchanged in this PR (rendering the count is `econ`'s or `ui`'s).
  - **The predicates made public** (item 133 (3) (j)): `matches_pick`, `on_candidate`,
    `generator_on` and the sphere test, each a documented `pub fn`, for the gateway to call.
  - **Two pure economy reads for `econ`** (item 134 (2) (c)): `economy::band_percent(rules, rank,
    living)` and the ladder-rank function `settle_ledger` computes inline (`world.rs` ~3670–3738,
    beside `economy.rs`'s `bmi_for` ~290), so the recap's `Settlement.band_rank` and
    `band_percent` come from the sim's rule rather than a restatement. Pure: no state change, and no
    chain moves; added beside `settle_ledger`, which is tick-path determinism code and stays as it
    is, with a test that each read agrees with what `settle_ledger` computes (`settle_ledger`'s rank
    counts from 0 and `band_rank` from 1, which `econ` converts, checked).
  - **Every silent fallback in `targeting.rs`**, each a typed or checked read (the global
    no-silent-fallback rule): `sphere_radius()`'s `unwrap_or(0)` (~157); the
    `usize::try_from(..).unwrap_or(0)` table-length reads (~184–268); `Ranker::new`'s origin and
    anchor reads (~427–428, ~444–445); and the same idiom in `spiral_offsets`, `covering_site` and
    `ring_key` (~531–532, ~561–583, ~622–623). `build` sweeps the sites item 133 (5) names.
- **Implements:** item 133 (3) (c), (h), (j); item 134 (2); targeting.md's "Sites" and "Surfaces".
- **Needs:** `tgtw` merged (it has). **Acceptance:**
  `a_named_vent_outside_the_sphere_is_no_target_whatever_stands_on_it` (a live Generator of
  another seat on it, and none: both `no_target`); the same for `covered {}`;
  `a_named_vent_inside_the_sphere_with_a_generator_is_still_illegal_site`; a named vent whose
  anchor point is inside the sphere and whose free `on` column is outside it is `illegal_site`; a
  `step_failed` value that decodes to its reason and its count, a bare reason id that decodes to
  the reason with no count, a value that does not decode refused with a typed error, and a gateway
  test that the line's words are unchanged; the economy reads agree with `settle_ledger`'s credit
  on the settlement golden; `cover-nearest-vent`'s round 2 still fails `illegal_site` (id 6). **No
  chain moves:** events are derived output, never hashed (`world.rs`; `crates/sim/tests/runner.rs`'s
  `the_event_bus_is_not_in_the_state_encoding`), and no committed run binds a named or `covered {}`
  `on` outside the sphere. The event-log goldens (`expand-east-segment`'s `expected.events.txt`, the
  `interpreter/guards` transcript) should not move, since bare ids decode as themselves; any that
  moves (a count landing in a value) is re-blessed with the line named. **Contract PR:** no.
  **PLACEHOLDERs:** none expected. **Agent-days:** 1–1.5.

### `grid` — named place in `crates/sim`: the four grid rulings

- **Owns:** `crates/sim/src/power.rs`, the grid tests in `crates/sim/tests/economy.rs`, and the
  `program_for` doc lines in `crates/sim/src/programs.rs`, and its scenario's path constant and
  `SCENARIOS` entry in `crates/gamectl/tests/scenarios.rs` (item 129; it merges after `mine` and
  rebases over that file); runs beside `fog` (AGENTS.md §6 named place), merging independently of
  `fog` and `econ` (item 134 (1); it was planned beside `mine`, which has merged). **Branch**
  `feat/sim-grid-rulings`, worktree `../pharmakos-grid`, target `D:/build/grid`.
- **Builds:** S1-33 (a shed that relieves nothing is skipped); S1-22 (a priority raise re-applies
  the brownout order: a dark beacon relights at once when shedding lit beacons of lower priority
  covers it, with draw no higher than supply after the swap), a check inside `settle` (`power.rs`
  ~190): a dark beacon whose load fits once lit lower-priority beacons are shed swaps with them, no
  new state, ties to the lowest seat and then the lowest beacon id (S1-22's copy in
  `crates/operator/src/safe.rs` is `oper`'s); S1-35 reworded in the `program_for` doc lines only
  (the behaviour exists, `programs.rs` ~131–146, and the new test pins it; the rules-text sentence
  of item 127 (5) rides with `tune`'s rules PR, because `rules/rules.v1.json` moves `rules_hash` and
  ships); S1-36 reworded as ruled; S1-31 per decision 12; `power.beacon_base_draw_kw` read:
  `PowerRules::of` reads it and a test asserts that a beacon's key-core supplies exactly that base
  (net zero holds by construction, decision 12), and no column changes (retiring it in favour of
  `structures.beacon.draw_kw`, if wanted, goes to `tune` as a contract row: a `rules.proto` change);
  and a `pub` read of a seat's dark load and shed kW in `power.rs`, pure, for `econ`'s shortfall
  line (`Shortfall.kw`; item 134 (2) (c)); `PowerRules::of`'s `map_or(0, ..)` row reads become
  typed reads that refuse a missing row (the global no-silent-fallback rule).
- **Implements:** item 127 (5) to (8); register S1-22, S1-31, S1-33, S1-35, S1-36.
- **Acceptance:** `a_beacon_whose_shed_relieves_nothing_is_still_shed_ahead_of_the_core` inverted to
  `…_is_skipped_and_stays_lit`; the raise test at `tests/economy.rs` (~481) flipped to
  `a_raise_relights_a_dark_beacon_when_a_lower_shed_covers_it` and its no-cover twin;
  `the_commander_walks_through_a_blackout_drawing_nothing`; new scenario
  `scenarios/s1/brownout-by-recycle` (recycling a Generator's beacon); **no committed chain moves**
  (none has a deficit) — if one does, the PR says which rule moved it.
- **Needs:** `tgt` merged (it moves `one_vent`); decisions 12 and 18. **Contract PR:** no.
  **Agent-days:** 2–3.

### `econ` — `crates/gateway`: the economy's surfaces

- **Owns:** `crates/gateway` (except `fog`'s named place in `strings.rs`), and named places in
  `crates/gamectl/src/scenario/run.rs` and `crates/gamectl/src/doctor.rs` (their Lull passing).
  **Branch** `feat/gateway-economy`, worktree `../pharmakos-econ`, target `D:/build/econ`.
- **Builds:** `get_recap`'s settlement lines (BMI, band, the award fund named as S4's) and the
  shortfall line (X-16): `Settlement.band_rank`, `band_percent` and `Shortfall.kw` (`gateway.proto`
  ~619–645) have no sim read on `main`, so they are filled from `fog`'s pure economy reads and
  `grid`'s shed read once `fog` and `grid` merge (the main session merges both first and `econ`'s
  fix pass rebases; if they have not merged by then, the main session adds that commit at the
  merge), and `econ` never restates a sim rule; the BMI line, from the `settled` event's value,
  ships regardless (item 134 (2) (c)); the standing sentence and the briefing's rank and score from
  `fixs`'s audit (X-03, over the existing `Standing` fields); the forecast per decision 12 (S1-12,
  S1-46); S1-11's "no countdown", filling `con2`'s `Status` field; the gateway's per-round Lull
  bound from `first_lull_ms` (before the client learns it): today `report_host_clock` refuses a
  remaining time above `lull_ms` (`surface.rs` ~1011–1012) and the Lull's elapsed time is `lull_ms`
  less the remaining (~1469–1525), so both read round 1's length from `first_lull_ms`, each a typed
  read rather than today's `map_or(0, ..)`. Then `gamectl scenario run` and `seat doctor`, which
  pass each Lull's whole length to `Surface::set_phase_remaining_ms`, pass round 1's as
  `first_lull_ms`. Carried by item 130: the stale `reserved 2 to 15` comment in
  `surface/knowledge.rs` (~173), and the recap's "Round N ran 0 ticks" for a full round once the
  match has ended, which `check` found. Carried by item 133: `resolve_refs` answers `PHASE_CLOSED`
  in a Push, as `estimate_route`'s `covering` does, with the test
  `resolve_refs_in_a_push_is_phase_closed` (item 133 (3) (f); the sim's fog follow-up does not cure
  the live read); `single` in `src/targeting.rs` gets the sphere test the sim's fix adds, with a
  test of a named vent outside the sphere (item 133 (3) (g)): the vent's anchor column's standing
  point within the sphere radius of the centre, through `fog`'s public sphere predicate once `econ`
  has rebased onto `fog`; if `fog` has not merged when `econ`'s fix pass runs, the main session adds
  that commit at the merge, as it does the predicate copies (item 134 (2) (b)); the recap's "3
  matched, none reachable" line once the fog follow-up puts the count on `step_failed` (item 133 (3)
  (h)), unless `ui` takes it; `tgtw`'s review B nits in `tests/targeting.rs` (the row-order test
  compares no picks with the sim's, the vent test the feature and not the column);
  `surface/planning.rs`'s module doc, which still calls FULL's estimate and lint stages "present and
  empty", and `tests/confinement.rs`'s message placing the one `Plan::compile` call in
  `compile_playbook` (it is `compile_decoded` now); and, once the fog follow-up makes
  `matches_pick`, `on_candidate` and `generator_on` public, `src/targeting.rs` drops its restated
  copies (item 133 (3) (j)): if `fog` has merged when `econ`'s fix pass runs, that pass rebases and
  drops them; otherwise the main session adds the commit at the merge (item 134 (2) (b)). `econ`
  leaves `strings.rs`'s `step_failed` line and its tests to `fog`'s named place.
- **Implements:** item 127 (2)'s gateway half; register S1-11, S1-12, S1-46, X-03, X-16.
- **Needs:** `tgtw` merged; decision 12. **Acceptance:** method and walkthrough goldens; a test that
  the bound admits a 600 000 ms first Lull and refuses 600 001; `scenario run` and `seat doctor`
  pass round 1's Lull as `first_lull_ms`; no chain moves. **Agent-days:** 4–5.

### `oper` — `crates/operator` + `library/`: Easy and the templates under S1's rules

- **Owns:** `crates/operator`, `library/`, `tests/golden/operator`, and a named place for a new
  `crates/gamectl/tests/wizard.rs` (item 124 (5) (l)). **Branch** `feat/operator-s1`, worktree
  `../pharmakos-oper`, target `D:/build/oper`.
- **Builds:** the templates rewritten with descriptions and without positional wizard parameters;
  Easy emits names and calls `estimate_route` with `covering`; `site_for`, `mine_site`'s heuristics
  and `SPHERE_MARGIN_VOXELS` deleted; `safe.rs`'s raise under S1-22's new meaning; S1-14 to S1-21
  per decision 14; the wizard's live `instantiate_template{suggested: true}` answer, hosted from
  `crates/gamectl/tests/wizard.rs`, gets its report and prose golden. Carried by item 133 (3)
  (a): Easy's and the safe playbook's Mine blocks write a `dig_max_depth`, so a seat nobody edits
  keeps earning past round 2 (F2's cure is the mechanism; the economy golden's seats 0 and 1 stop
  after round 2 at the default depth 0). The main session runs `cargo
  xtask package` locally before merging (`library/` ships).
- **Implements:** item 124 (5) (l), item 127 (7), (12) (Easy's names and the templates); register
  S1-14 to S1-21.
- **Needs:** `tgtw` and `grid` merged; decision 14. **Acceptance:** the operator goldens,
  `instantiate_suggested` and the `demo_*` render and verify goldens moved and explained; the
  wizard's new golden; the templates round-trip, verify and render identically; the `against-easy*`
  chains re-blessed in this PR only if Easy's sealed files change (they do; the reason is named).
  **Agent-days:** 3–4.

### `ui` — `crates/client-gdext` + `godot/`: the economy and targeting UI

- **Owns:** `crates/client-gdext`, `godot/`. **Branch** `feat/client-s1-ui`, worktree
  `../pharmakos-ui`, target `D:/build/ui`.
- **Builds:** the first Lull's countdown from `first_lull_ms` (`rig.rs` `set_lull_length`,
  `bridge.rs`); the recap's settlement, shortfall and "found nothing" lines; the Lull's "this
  round:" line; the chip; a click on a vent offering "Place beacon covering this vent" and Alt-click
  turning it into the description; E0601's `allow_dormant_beacons` checkbox; S1-44 per decision 17.
  Carried by item 133: the "this round" sentence has no field; it rides `get_briefing`'s prose in a
  Lull, for a seat's own token only, and this lane shows it from there; it gets a field when v1.1
  publishes the method (item 133 (3) (i)). The recap's "3 matched, none reachable" line is this
  lane's or `econ`'s once the fog follow-up puts the count on `step_failed` (item 133 (3) (h)).
- **Does not:** decide, time or validate anything (AGENTS.md §3 rule 4): every answer is the
  gateway's.
- **Implements:** item 127 (2)'s client half, (12) (the chip, the click and Alt-click); register
  S1-44.
- **Needs:** `econ` and `tgtw` merged; decision 17. **Acceptance:** vista and rows PNGs re-rendered
  on CI's Linux leg and looked at; `godot_project.rs` pins; the main session's local package run.
  **Agent-days:** 4–5.

### `build` — `crates/sim`: Build settings and S1-42

- **Owns:** `crates/sim`, and named places in `crates/verifier/src/hash.rs` and the gateway's
  fingerprint caller (edited only after `econ`, the gateway's owner, merges: section 4.1).
  **Branch** `feat/sim-build-settings`, worktree `../pharmakos-build`, target `D:/build/build`.
- **Builds:** the Build settings decision 5 keeps (`order`, `rotation_quarter_turns`,
  `protected_areas` kept clear of construction); `BUILD_HP_PER_SECOND` read from its row; S1-42
  (`hash::plan_fingerprint` in the sim, the value unmoved, the verifier's and gateway's callers
  switched). No new spending failure: the Build mandate builds "the highest-order affordable target"
  (spec §6, `mandate.rs`) and a Build row "is not a spend" (`exec.rs`), so only the deploy fails a
  step for want of `$` in S1 (decision 7); `queue_structure` is S4's.
  Carried by item 133: the sim's interface pricing brought to decision 15 (register S1-51; item
  133 (3) (b)): a Build target its own 2.5 s row rather than one settings field for the list, and
  each element of `protected_areas` or `probe_areas` one field (`mandate_fields`), re-blessing the
  chains it moves; when the sim stops counting a target list as a field, a `set_mandate` whose
  Build arm holds only targets must still compile to carried settings, or the gateway's
  `resolve_playbook` count guard refuses those playbooks (`INTERNAL`); and the silent fallbacks of
  the skeleton's idiom, each a typed or checked conversion or a destructured anchor:
  `set_beacon_scouts`' `u8::try_from(..).unwrap_or(u8::MAX)` in `interpreter/exec.rs` (a typed
  refusal, or a verifier error), the same file's `point_of` and anchor reads (~1700–1721) and its
  `usize` row-index conversions (`restart_row`, `visit_row`, `fallback_leg`, ~615, ~889, ~1420);
  `interpreter.rs`'s `Row::duration_ms` (`extra`'s `unwrap_or(0)`, and a missing `interface_times`
  block read as 0); and in `mining.rs` `point()`'s `i16::try_from(..).unwrap_or(0)`, `pillar()`'s
  anchor read and the file's other `unwrap_or(0)` reads. Carried by item 134: the scenario notes
  that say a failure's id "is pinned by the chain" are untrue, because events are never hashed
  (`cover-nearest-vent`'s header, `against-easy-three-rounds`' round-2 note (~90) and
  `unaffordable-deploy`'s header); `build` rewords them when it next touches the scenarios. Carried by item 134 too: `economy.rs`'s
  `bmi_for` (~290) reads its rows with `unwrap_or(0)` and falls back to `Money::ZERO` when the
  economy block is missing; `build` makes each a typed read, in the same sweep.
- **Implements:** item 33 (a)'s Build settings as decision 5 reads them; register S1-24's reader,
  S1-42; spec §6's Build row.
- **Needs:** `mine` and `fog` merged (one author of `crates/sim`); its fingerprint-caller commit
  after `econ` merges (section 4.1); decision 5. **Acceptance:** `targets_build_in_their_order`;
  `nothing_is_built_inside_a_protected_area`;
  `unaffordable_targets_wait_for_the_highest_order_affordable_one` (T14 did not pin it);
  `the_plan_fingerprint_is_unmoved`; chains with a Build re-blessed with reasons; every
  `report_hash` golden re-blessed if it adds hashed state.
- **Contract PR:** yes in part (the fingerprint moves into the hash module; its value must not
  move). **Agent-days:** 4–5.

### `tune` — the numbers, and the real determinism segment

- **Owns:** `rules/rules.v1.json` values, `crates/gamectl/tests/balance.rs`, `xtask/src/main.rs`
  (`DETERMINISM_TICKS` and the `placeholders` step), `tests/golden/determinism`, and named places
  for S1-26's anchors: `crates/sim/src/lib.rs` (`DETERMINISM_SEGMENT_LENGTHS_MS`,
  `determinism_playbook`, `DETERMINISM_UNITS_PER_SEAT`), `crates/sim/src/world.rs`
  (`fill_unit_table`, `WorldConfig::units_per_seat`, `draw_new_destinations`),
  `crates/sim/tests/determinism.rs`, `crates/sim/tests/allocations.rs` and `crates/sim/src/bin`.
  **Branch** `feat/rules-s1-numbers`, worktree `../pharmakos-tune`, target `D:/build/tune`.
- **Builds:** runs `check` on the finished rules; proposes each economy row per decision 16 with a
  PLACEHOLDER naming the owner at S1's demo; S1-26 per decision 16: `DETERMINISM_TICKS` raised to a
  real segment and the harness segment list, playbook and fifty walkers deleted; `cargo xtask
  placeholders` made a `ci` step once the lanes have reworded their markers (decision 9); decision
  16's `expand_east` column in `balance.rs`'s `render`, which `check` left as a PLACEHOLDER (item
  130), read from the committed `expand-east-segment` scenario's run, or, if it needs a second
  harness door, through a named place granted here first. The check's seed set and flag readings are
  the owner's at S1's demo, with this report.
  Carried by item 133: the depth `oper` gives Easy and the safe playbook is tuned with the numbers
  (item 133 (3) (a)); and `rules/README.md`, which ships, is reworded here, the next lane to touch
  `rules/` (packaged locally by decision 9): `economy.mining_carry_voxels`' row still describes
  the sim's deleted `MINING_LOAD_VOXELS`, and `interface_times.*`' Settled? cell becomes
  "ratified, decision 15" (U-04). Carried by item 134: S1-35's rules-text sentence (item 127 (5),
  the commander walking through a blackout drawing nothing) rides with this lane's rules PR, since
  `rules/rules.v1.json` moves `rules_hash` and ships; `grid` rewords only the `program_for` doc
  lines.
- **Implements:** item 127 (3); item 103 (3)'s walk speed; register S1-26 and the economy rows'
  owner PLACEHOLDERs.
- **Needs:** `build`, `oper` merged; decision 16. **Acceptance:** the check's report attached to the
  PR; every chain and `report_hash` golden re-blessed once, the rows named. **Contract PR:** yes
  (determinism harness, `xtask`; `rules.proto` only if a row retires). **Agent-days:** 2.5–3.

### `demo` — the run sheet, the P1 run and the register

- **Owns:** `docs/demo/s1-run-sheet.md` (the main session's docs PR), the register's S1 section.
  **Builds:** the P1 certifying run on the owner's machine; the 10k fuzz run through
  `nightly-scenarios.yml`'s manual `force` input, recorded (section 5); the run sheet; the
  register's S1 rows closed or moved with their owners. Carried by item 133: the P1 run measures
  FULL against a late-match snapshot, since FULL decodes the whole snapshot on every call for
  `coming_segment_ms`; moving the length into `Scope` is a `report_hash` contract change and a
  gateway change, the owner's with P1's FULL budget. **Implements:** AGENTS.md §10 items 6 and 8;
  item 33 (c). **Needs:** `tune` and `ui` merged. **Agent-days:** 1.5–2.

---

## 4. The schedule

### 4.1 Waves

Three slots, never more. `crates/sim` has one author at a time; beside it run only named places:
`con2`'s `compile_place` and its new `tests/targeting_refused.rs` during `fixs`, and `grid` during
`mine`, then beside `fog` (item 134). `tgt` carries P1's evaluation-units counter and `mine` its
seam estimates, so `p1` has no place in the sim. `build`'s named place in the gateway's fingerprint
caller overlaps `econ`'s ownership of `crates/gateway` (days 18.5–23): `build` edits that caller
only after `econ` merges, as its last commit. Days are working days in plan units, five to a week; a
task starts the day its inputs merge. A lane opens its PR when it is green (`wave-lanes.js` holds
none back), so the order inside a wave is a **merge** order: `fixs` and `fixc` merge after `con1`,
the merge train rebasing them (both are `MATRIX_ONLY`, so they re-run the matrix), and `con2` after
`fixc`. `proj`'s rebase and re-bless over `tgt` is a separate half-day pass, placed in wave 3's
third slot so that no fourth agent runs beside `mine`, `tgtw` and `grid`.

| Wave | Days | Slot 1 — sim | Slot 2 — gateway, verifier, operator | Slot 3 — contracts, client, harness | The owner can see |
|---|---|---|---|---|---|
| 1 | 0–6.5 | **fixs** (6) | **fixc** (3.5) → **check** (2) | **con1** (3.5) → **con2** (3) | First clicks land; $ 60 charged; 10:00 in the rules; the first balance report |
| 2 | 6.5–15.5 | **tgt** (9) | **tgtv** (2.5) → **proj** (5, open until `tgt` merges) | **p1** (4.5) | E0601/W0602 in the editor; the P1 notices |
| 3 | 15.5–23 | **mine** (5.5) | **tgtw** (3) → **econ** (4.5) | **proj**'s re-bless (0.5, then it merges) → **grid** (2.5, named place, 16–18.5) → **oper** (3.5, from 18.5) | Covering the nearest vent; mining past round 1; a brownout by recycling |
| 4 | 21–27.5 | **build** (4.5) | — | **ui** (4.5, from 23) | The chip, Alt-click, recap lines, the 10:00 first Lull |
| 5 | 25.5–30.5 | **tune** (3) → **demo** (2, from 28.5) | — | — | The S1 demo of §1.1 |

**As run (item 131 (6)).** `proj` did not run as wave 2's second run in slot 2: it builds in full
in wave 3's slot 3, beside `mine` and `tgtw`, and with `tgt` merged it needs no separate re-bless
pass. `grid` follows it in slot 3, as its named place if `mine` is still open and otherwise after
`mine` merges, then `oper`; `econ` follows `tgtw` in slot 2.

**As run (item 133).** Wave 3's run built `proj`, `mine` and `tgtw` at once; they merged in that
order on 2026-10-06 (#85, #86, #84), `mine` re-blessing `report_hash` over `proj`, and `tgtw` over
both, where the commander's voxel in `Scope` let `proj`'s estimate count each route's first leg, so
two `demo_*_verify` goldens moved in their I0001 as well as their hash (section 4.2's `tgtw` row
said the gateway's hashes only). One lane joins the schedule: the sim's fog follow-up of item 133
(3) (c), a small `crates/sim` lane in slot 1 ahead of `build` (a named or `covered {}` `on` whose
`on` column lies outside the target sphere answers `no_target` before the structure test), with
`grid` beside it as its named place in slot 3, then `oper`; `econ` runs in slot 2. The follow-up
also puts the candidate count on `step_failed` and makes `matches_pick`, `on_candidate` and
`generator_on` public (item 133 (3) (h), (j)). Each launches on the owner's word. The table above is
superseded where the as-run notes differ: it is the plan as adopted, and these notes are the
schedule.

**As run (item 134).** On the owner's word ("resume", 2026-10-06) wave 4's first run launched three
lanes at once on `main` at `46ad80c`: `fog` (section 3, slot 1), `econ` (slot 2) and `grid` (slot 3,
its named place beside `fog`). Merge order `fog` → `econ`, with `grid` independent of both: `fog`
holds a named place in `econ`'s `crates/gateway/src/strings.rs` (`step_failed`'s line), and `econ`
drops its restated predicates only once `fog` has made them public. `oper` follows `grid`; `build`
runs after `fog` (one author of `crates/sim`), its gateway commit after `econ`.

**Totals.** 18 tasks in 17 lanes (`tgtv` and `proj` are one lane in two PRs), **about 71.5 ad**
(63.5–79.5), against the code map's 62–74; this plan adds X-08's audit, the restore bug, the
selector bug, S1-26 and S1-42 to it, and the reviews added `con1`'s and `con2`'s named places and
`tgt`'s harness re-site. Adopting `toward` adds 2–3. **Critical path:** `con1` → `con2` → `tgt` →
`mine` → `build` → `tune` → `demo`, **about 30.5 working days**: `con1` → `con2` (6.5) now outlasts
`fixs` (6) beside it, and `tgt` waits for both. Slot utilisation is about 78 % of 91.5 slot-days.
Two splits already shorten it: targeting's preview surfaces (`tgtw`) leave the sim PR and run beside
`mine`, and the check is built in wave 1 so `tune` is three days, not five.

**Token cost.** At waves 4 and 5's 186–195 k sub-agent tokens per planned agent-day (item 115 (2)),
71.5 ad is about **13.3–13.9 M** tokens; at wave 6's 215–245 k it would be 15.4–17.5 M.

### 4.2 The re-bless ledger: how many times the chains move, and why

| PR | Scenario chains | Determinism chain | `report_hash` goldens |
|---|---|---|---|
| `con1` (rules rows) | — | — | all (`rules_hash`) |
| `fixs` (X-12; X-08's audit) | all four of the skeleton's (each places a beacon), and its two new ones; a chain that reaches a round-limit end also hashes the audit's winner in the existing winner byte | **moved** (the harness's deploy step pays `$` 60) | — (X-08 adds no state: `winner` stays `Option<SeatId>`, `SNAPSHOT_VERSION` stays 6) |
| `tgtv` (`Scope`'s layout) | — | — | all |
| `tgt` (ids, hashed bindings, sites) | all | **moved** (ids, bindings, the harness deploy re-sited) | all (the snapshot version, ids); `tgt` owns the re-bless of `tests/golden/verifier` and `tests/golden/gateway/*_verify` |
| `tgtw` (`Scope`'s content) | — | — | the gateway's |
| `proj` (new codes) | — | — | all, if it moves the verifier version (the reports' `diagnostics` in any case) |
| `mine` | all | **moved** (the harness's Mine beacon digs past the rim) | all, if the held seam bumps the snapshot |
| `grid` | none committed | — | — |
| `build` | those with a Build | — (the harness writes no Build target) | all, if it adds hashed state |
| `oper` (Easy's sealed files) | `against-easy*` | — | the operator's and the `demo_*_verify` goldens |
| `fog` | none; event logs only if a count lands in a value (`expand-east-segment`'s, the guards transcript), and with bare ids decoding as themselves none is expected | — | — |
| `tune` (values; S1-26) | all | **moved** | all |

**Over the stage the scenario chains move six times** (`fixs`, `tgt`, `mine`, `build`, `oper`,
`tune`), **the determinism chain four times** (`fixs`, `tgt`, `mine`, `tune`), and every
`report_hash` golden four times for certain (`con1`, `tgtv`, `tgt`, `tune`) and up to three more
(`proj`, `mine`, `build`). X-12 and targeting together move the scenario chains and the determinism
chain twice. Folding X-12 into `tgt` would save one determinism re-bless and one scenario re-bless,
but it delays a fix the owner put in the first lane (item 126 (2) (f)) by about nine plan-days, to
`tgt`'s merge at day 15.5, and makes the stage's largest PR larger, so it is not recommended. Each
move is one behaviour change reviewable alone, `mine` and `build` together would pass the 10-ad
split rule, and the numbers must come last because the check needs the finished rules. S1-26 lands
in `tune`, not `tgt`: the real segment's longer chain would otherwise be re-blessed at `mine` too,
over more ticks.

### 4.3 Honest size, and the options (decision 1)

About 71.5 ad and a 30.5-day critical path in plan units, against the spec's 5 weeks (25 days); with
a week of float for contract reviews, about 7.1 weeks. The sim alone carries about 25 ad in series
(`fixs`, `tgt`, `mine`, `build`). Item 127 (9) and (12) named this downside when they were ruled.

Plan units are not calendar days: the skeleton's 204.5-ad plan merged about 14 to 18.5 ad a day in
waves 5 and 6 (item 115 (2)), and its stops, the Windows CI leg (about 51–55 min since items 123 and
124) and serial docs PRs set the calendar. `scripts/merge-train.sh`'s `MATRIX_ONLY` sends back to
the three-OS matrix every lane that touches `crates/sim/`, `crates/mesher/`, `crates/client-gdext/`,
`godot/`, `library/`, `rules/`, `packaging/`, `LICENSES/`, `REUSE.toml`, `.github/`,
`xtask/src/package.rs` or `zip.rs`, the root `Cargo.toml`, `.cargo/`, the chain and geometry goldens
or `tests/golden/package/`, so 12 of the 18 PRs (`fixs`, `fixc`, `con1`, `con2`, `tgt`, `p1`,
`mine`, `grid`, `oper`, `ui`, `build`, `tune`) pay a second full matrix whenever their base moved
(item 116 (2)).

### 4.4 How lanes run in S1

- **`wave-lanes.js` is pointed at this plan before wave 1** (item 126 (5)), by the main session in a
  small `.claude/` PR on the prose fast path: a new `args.plan` (default `docs/design/s1-plan.md`)
  and `args.stage` ('S1') replace the hard-coded "skeleton-plan.md section 3" and "the walking
  skeleton" in `brief`, `reviewPrompt` and lens B; the builder reads section 3's task section of
  `args.plan`, found by the backticked id (the headings read "### `fixs` —" and "### `tgtv` → `proj`
  —", so the brief's `### ${lane.task} ` match changes to one on the backticked id), then section 2
  of that plan and the decisions sections 6 and 7 name. Lane `task` ids are this plan's (`fixs`,
  `fixc`, …). More lines change: (a) each lane's required `items` holds the §2.7 items of its task's
  Implements line, which every task in section 3 now carries, plus item 128, and the brief's step 2
  adds "and the register rows the Implements line names, in `docs/placeholders.md`"; (b) the
  environment's sentence that `cargo xtask determinism --bless` is "ONLY for the sim lane" becomes
  "only for a lane whose plan section says the determinism chain moves" (S1: `fixs`, `tgt`, `mine`,
  `tune`, the last not the sim's owner); (c) the environment's rule against "adding a new crate to
  the workspace list" gains an exception for `p1`, which adds `crates/bench` (decision 2). No PR
  hold is needed or exists: the builder opens its PR when green, and section 4.1's order is a merge
  order. A run still launches all its lanes at once, so each `→` in section 4.1 is a separate run
  (wave 1 alone is two: `fixs`, `fixc` and `con1`, then `con2` and `check`).
- The main session's docs PRs carry the spec amendments (targeting's list; item 127 (5), (6), (7)'s
  "priority … orders brownouts and nothing else" in §5 and §7, (8)), the AGENTS.md sentences (§3's
  crate-map row for `pharmakos-bench`, §3 rule 1's "any of the five", §3 rule 2's previews, §3 rule
  4's `rig.rs` keep-alive sum per decision 17, §5's reserved-range discharge, §4.9's walled list,
  its "the research guard's five crates plus the sim" and its `CLIENT_WALL` sentence, §9's
  local-package sentence per decision 9, §9's line on a check run by hand (`#[ignore]`d, never a
  step, never compared across operating systems; item 130), §9 item 11's P1, and, from item 131,
  §3 rule 2's "never ranks" and §4.8's derived feature table, and from item 133 §3 rule 2's
  `resolve_refs` preview, `compile_decoded` and the verifier's lower-bound travel, the crate map's
  plan-core and verifier rows, and §9's bless note) and the rulings, and
  launch each run through `docs_ref` (item 115 (3)). AGENTS.md is a harness doc no lane edits: the
  `pharmakos-bench` sentences are in the docs PR that lands with `p1`, whose PR rewords the
  matching `xtask` doc comments. `rules/README.md` is `con1`'s, not theirs.
- A lane that changes any of AGENTS.md §9's six shipped paths (`library/`, `rules/`, `packaging/`,
  `LICENSES/`, `REUSE.toml`, `godot/`) is packaged locally by the main session before its merge
  (decision 9's reading of item 124 (5) (c)): in S1, `con1`, `fixc`, `oper`, `ui` and `tune`, and
  any lane given a named place under `godot/`.

---

## 5. P1, the depth task

**The criterion, verbatim from spec §16's P1 row.** When: "QUICK at S1; FULL and the size budget are
S3's exit criterion". Go if: "QUICK ≤5 ms p99; FULL ≤50 ms p99 at the size budget; byte-identical
across operating systems; 10k fuzzed playbooks with no panic. The per-rule cost on a decision tick
(one per 250 ms of game time) is measured against the 50 ms sim tick to set the playbook size
budget". Fallback: "Smaller budget; debounce".

**What S1 certifies** (item 33 (c): "QUICK plus per-rule cost per decision tick at slice 1"):

1. **QUICK ≤ 5 ms p99** over the committed verifier cases and `budget_128`, on the finished S1
   verifier (after `tgtv` and `proj`, which add resolve and semantics work), in a recorded run on
   the owner's demo machine (i7-9800X), with the three CI runners' figures published beside it as
   notices.
2. **Byte-identical reports across OSes**, which the verifier goldens already check on every PR.
3. **The per-rule cost per decision tick,** measured and recorded as the input S3 sets the size
   budget from (item 94; `verifier.size_budget_units` stays 128 until S3's exit). The unit: counted
   evaluation units per decision tick per seat (handlers tried, condition nodes, selector
   candidates, "nearest" estimates — targeting's ranking cost included), asserted in the sim as
   counted work (`tgt`'s unhashed counter, reusing `seams::WorkCounter`; `mine` adds its seam
   choice's estimates), and the wall-clock cost in ns per size unit, measured by the walled crate
   (`p1`) stepping a hosted match whose playbooks sit at the size budget. The report states the
   implied budget at a 50 ms tick with three seats.
4. **The 10k fuzz:** run once through `nightly-scenarios.yml`'s manual `force` input at S1's close
   and recorded, by `demo` (the nightly schedule stays off until S2). FULL ≤ 50 ms and the size
   budget stay S3's.

If QUICK misses, the written fallback is taken: a smaller provisional budget and a debounced QUICK
in the editor, recorded in the demo record, with the owner's word.

**The harness needs a new walled crate (decision 2, owner, contract).** No crate may time the
verifier today: the verifier, sim and gamectl read no clock (AGENTS.md §4.5); `client-gdext` may not
reach the verifier (`CLIENT_WALL`); `mesher` may not reach the sim; `xtask` is dependency-free. Item
116 (6) (b) names adding one as the owner's. The new crate takes no `CLIENT_WALL` line: that list
names what a walled crate may never reach, and this one exists to reach the sim, as §4.5's walled
harness does (`godot` is no workspace member for a line to name). `p1` rewords the `xtask` doc
comments that say every walled crate takes a line, and the main session AGENTS.md's matching
sentences (section 4.4).

---

## 6. Owner decisions before the stage starts

Recommendation first, marked **(Recommended)**, downsides for each option. **[play]** marks the
play-shaping ones the main session interviews on (item 127 (1)); the rest pass in bulk. **All ruled
as recommended, 2026-09-29 (item 128).** As it happened, decisions 1 and 2 were interviewed although
not marked, and decisions 4, 16 and 18, marked **[play]**, were put to the owner by name in the bulk
question rather than interviewed.

**1. S1's size and schedule.** *Governs everything; answer with this plan's summary.*
- **(Recommended) Keep every ruling and let S1 run about 7 plan-weeks (to about wk 25.5),** with
  deferrals as the only cuts: decision 12's what-ifs and `toward` (decision 3) reverse nothing, and
  decision 5's is ruled on its own. *Downside:* later rows shift about two plan-weeks unless
  absorbed; the gate date is the owner's.
- *Move named items to S2 or S3:* pillar spacing and SAFEST (≈1.5 d of the sim path), the final
  audit X-08 (≈1.5 d, off the critical path, which `con1` → `con2` now sets in wave 1), P1's
  wall-clock half (≈4.5 d, off the critical path). *Downside:* the first two reverse item 127 (9)
  and X-08; the third leaves the stage's named depth task unmet.
- *Three lanes in the sim (`build` beside `mine` by named places):* about 4.5 d off the critical
  path. *Downside:* two agents in `world.rs` and `interpreter.rs` at once, on the determinism crate,
  where a bad merge is a desync; it stretches AGENTS.md §6's named place past its intent.

**2. P1's harness.** *Gates `p1` (wave 2). Contract (§4.9), owner-named by item 116 (6) (b).*
- **(Recommended) A new walled crate `crates/bench` (`pharmakos-bench`, `publish = false`, never in
  a zip),** in `WALLED_PACKAGES` only, with no `CLIENT_WALL` line (section 5 says why), and added to
  `GUARDED_PACKAGES` so it never enables `research`; `WALL_GUARDED_PACKAGES` already keeps every
  deterministic crate off it. It depends on the sim with `default-features = false`, the verifier
  and the gateway; reports through `cargo xtask perf-alarms` as per-runner notices, never a CI gate
  in S1; and is certified by the recorded run of section 5. AGENTS.md §3's crate map gains its row
  and §9 item 11 is amended to say so, in the main session's docs PR that lands with `p1`.
  *Downside:* a new workspace member and a wider wall list; a slow regression shows only as a notice
  until S3.
- *The same crate, with QUICK's p99 as a required CI gate:* *Downside:* shared runners vary, the
  three OSes differ, and a flaky red check is worse than none (the skeleton's R12).
- *Counted work only, no milliseconds:* legal today. *Downside:* it cannot answer "≤ 5 ms", so P1's
  S1 half would take its fallback by construction.

**3. [play] `toward`, a site arm for stepping-stone expansion (`Location` 12).** *Gates `con2`.*
- **(Recommended) Defer to S3, with the rest of the catalogue (`RICHEST`, filters), after a design
  pass like "nearest" had.** A carried "cover the nearest vent" then finds nothing after round 1 on
  the golden seed and says so (targeting.md's finding). *Downside:* in S1 a seat expands past its
  first vent only by a positional step re-authored each Lull, and Easy's stepping stones wait for S5
  (item 113 (7)), so the balance check sees little expansion.
- *Adopt it in S1:* +2–3 ad (proto, sim spiral, verifier, editor, Easy). *Downside:* irreversible
  vocabulary designed under schedule pressure, on a stage already over.

**4. [play] Overrule check on targeting's "nearest" and one structure per voxel.** *Gates `con2`.*
**(Recommended)** keep both as adopted. *Downside:* both are irreversible once files use them.
*Alternatives:* octile distance (rejected in item 127 (13): picks unreachable vents); a vent shared
by seats (a second tap gives 0 kW today, so a second Generator is wasted money).

**5. [play] Which Build settings "full" means in S1.** *Gates `con2`'s comments and `build`.* The
spec's Build row lists targets, repair threshold, rebuild destroyed, terraform, protected areas and
repair/reclaim drone fields; §17 puts repair/reclaim drones in S2; `playbook.proto` says fields 6
and 7 "arrive with the full Build mandate at S1"; the spec states no behaviour for terraform beyond
its enum.
- **(Recommended) S1 builds targets in full (order, rotation, `on`, `covered {}`) and protected
  areas; repair threshold, rebuild destroyed and terraform stay stored and priced, taking effect in
  S2, and the rule list says "from S2"; fields 6 and 7 stay reserved until S2.** This amends item 33
  (a) and spec section 17's S1 row, and leaves terraform's meaning unruled. *Downside:* it defers
  two of the three settings item 33 (a) named for S1 (terraform, repair threshold), reading "full
  Build settings" narrowly.
- *Terraform in S1 too,* on a meaning the owner rules (a proposal: a drone levels the target's
  footprint to its anchor's height before building; FILL raises, DIG lowers, BOTH both). +1.5–2 ad
  on the sim path. *Downside:* a new rule written now.

**6. [play] The four Mine settings' meanings.** *Gates `con2`'s comment and `tgtv`'s SAFEST warning
(waves 1 and 2), and `mine` (wave 3).* **(Recommended):** `dig_max_depth` counts voxels below the
seam's original top surface (0 = the rim only); `pillar_spacing` N leaves every Nth column in x and
y undug (0 = no pillars) — a dig pattern needing no collapse rule, which the spec does not state, so
this re-reads item 127 (9)'s conditional ("if pillar spacing needs S2's collapse rules, the verifier
warns") as not triggered, and no warning is needed; "never under structures" refuses a voxel in or
beside the footprint column of any live structure or beacon. `seam_choice`: NEAREST as targeting
defines it; **RICHEST** = the most remaining yield (ore voxels × richness) among reachable seams in
the beacon's sphere, ties by nearest — this is the Mine program's own setting, which targeting.md
does not defer (it defers `RICHEST` only as a `FeatureRef` rank); **SAFEST** reads as NEAREST until
S2's threat model, and the verifier warns (item 127 (9)'s pattern, `tgtv`). `flee_on_threat` is
stored and does nothing until S2. *Downside:* SAFEST does nothing distinct in S1; and without
collapse, pillars only cost yield, so a non-zero `pillar_spacing` is strictly worse until S2.
*Alternative:* SAFEST = farthest from the nearest known enemy asset. *Downside:* a threat model
invented before S2's.

**7. The step failure for an order the treasury cannot cover.** *Gates `fixs`.* §7: it "fails like
any other step"; ids 1–10 exist, 11 is `feature_lost`.
- **(Recommended) a new additive id 12, `unaffordable`,** for the deploy, the only step that spends
  in S1; the id also serves `queue_structure` at S4. *Downside:* one more code in the failure table.
- *Reuse `illegal_site` or `no_target`:* *Downside:* misleads `on_fail` and the recap.

**8. F5's wording.** *Gates `fixs`.* **(Recommended):** a gateway-filed seal reads "The Lull ran
out, so the safe playbook was filed for you." in that seat's own feed; `AUTHOR_BUILTIN` becomes
"Written from a built-in template." for every instantiated playbook (true for Easy's too).
*Downside:* the four `demo_*_render` goldens move. *Alternative:* stamp `author_kind: HUMAN` for a
human seat's instantiation. *Downside:* the meta then claims authorship of text the player may not
have touched, and `report_hash` goldens move too.

**9. S1's first contract PR (`con1`).** *Gates `con1`.* **(Recommended),** each item separable:
S1-01 pin 1.98.1 everywhere; S1-02 reword to G4's result; S1-05 the three fall-backs fail (it
changes what `ci` does); S1-06 `1` is on, unset or `0` off, anything else an error; S1-07 every
child through `child_command`; S1-08 the comment's premise corrected; S1-10 the grammar `//
PLACEHOLDER: <what> — <who>, <when>` and `cargo xtask placeholders` outside the step table, made a
`ci` step by `tune` (the stage's last `xtask` PR) once lanes have reworded their markers; item 124
(5) (c): the lanes' R5 stands and the main session packages locally, which AGENTS.md §9 then says;
(e) add the `REUSE.toml` override T22a drafted; item 125 (3) (b) waits for the owner's live check of
`claude.yml` (handoff NEXT 3) and (c) is recorded, unchanged; X-14 ratified; X-15 and X-17 named S3,
X-18 after S4 with M-09; M-05 takes none of item 97's three assertions now; M-03 per its paths.
*Downside:* S1-05's failures can redden `ci` on a metadata slip.

**10. [play] The new rows' values.** *Gates `con1`.* **(Recommended):** `first_lull_ms` 600 000,
`lull_ms` 300 000 (ruled by item 127 (2)); `build_hp_per_second` 60 and `mining_carry_voxels` 16
(today's constants); the minimum beacon spacing at "no stacking only" — the same column — which is a
legality rule in the sim, so S1 adds no spacing row; `revision` 3 → 4. *Downside:* a larger spacing,
if the owner wants one at the demo, is a new row in a later contract PR and moves every chain again.

**11. S1-39, how an omitted enum or setting reads.** *Gates `con2` and `tgtv`.* **(Recommended):**
the plan proposes defaults the spec does not state — `seam_choice` NEAREST, `pillar_spacing` 0,
`terraform` NONE, `pace` DIRECT (`Pace` has only DIRECT and AVOID_KNOWN_THREATS; `playbook.proto`'s
own PLACEHOLDER says the spec gives no default) — each with an I-level note; the spec's §10 example,
which omits two Mine fields, then verifies. Targeting's new `*_UNSPECIFIED` stay errors, as adopted.
*Downside:* four invented defaults (AGENTS.md §12), irreversible once files omit them, and two rules
for "unset". *Alternative:* unset is an error, and spec §10's example is amended to write the two
fields. *Downside:* the spec's own example changes, and every hand-written Mine block must spell out
all four settings. **M-14** (the mandate messages to `gp/v1/mandate.proto`): not in S1, because the
move changes no wire or JSON name (same package) and nothing in S1 needs it; the register row is
re-owned to the next proto PR that adds a mandate field. *Downside:* `playbook.proto` keeps growing,
and the later move is a churn-only PR in a contract file. **M-11**, its sibling: the interface row
catalogue is enumerated in `con2`'s `InterfaceStep` comment, as built.

**12. The economy surfaces.** *Gates `con2`'s wire (wave 1); `econ` and `grid` (wave 3).*
**(Recommended):** S1-31 keep the key-core netted out of draw and say so beside the meter; S1-11 a
distinguishable "no countdown", carried as a new `Status` field (`con2`) rather than a sentinel in
`phase_remaining_ms`, whose 0 every client already reads as "ran out"; S1-12 keep the live answer in
a Push (a seat's own economy is its own knowledge); S1-46 fill `bmi_next` and committed spend only,
what-ifs to S3. *Downside:* the forecast stays thin.

**13. Mapgen and sight constants (S1-24, S1-25, S1-27 to S1-30, S1-34).** *Gates `con1`'s rows and
`fixs`'s S1-34 wording (wave 1); `mine` (wave 3).* **(Recommended):** S1-24 and S1-25 become the
rows of decision 10; S1-27 to S1-30 stay constants, re-owned to S4's map re-derivation; S1-34's
sight radius 8 is S2's, with scouting, and its copies say so.

---

## 7. Owner decisions during the stage

**All ruled as recommended, 2026-09-29 (item 128).** A lane that finds one of them wrong in the code
raises it with the owner rather than building around it.

**14. Easy and the safe playbook (S1-14 to S1-21).** *(`oper`, wave 3.)* **(Recommended):** S1-14,
S1-16 and S1-20 are discharged by names and `estimate_route` (the beacon adds no net draw, per item
113 (5)); S1-15 Easy adds a Generator to an existing Build beacon's list when a covered vent lies in
its sphere; S1-17 to S1-19 keep §14's 60 s and 2 and the bound 4; S1-21 "power is short" = draw
above supply, "at risk" = lit and next in the shed order within `SAFE_REACH_MS`, now that a raise
relights. *Downside:* Easy's goldens move.

**15. [play] Interface pricing (S1-23, X-05, U-04).** *(`proj`, wave 2.)* **(Recommended):** each
changed element of a list counts as one field (the 6 s cap still bounds it); a deploy pays no
separate handshake; §5's times ratified as rows.

**16. [play] The economy's numbers.** *(`tune`, wave 5; the owner tunes at the demo.)* The spec
gives ranges for few rows (§7's +10 % / −5 %, §4's commander speed). **(Recommended):** a row with a
spec range stays inside it; a row without one keeps its item-90 seed value unless the check flags
one of its three conditions, and then the PR proposes a change with the flag as evidence. Walk speed
against map span, which item 103 (3) left to the owner at S1: the check's report adds whether the
spec's `expand_east` reaches its first site within its 120 s timeout, and the owner rules at the
demo whether speed (inside §4's range) or the example moves. *Downside:* the numbers are only as
good as Easy's play, which seals its safe playbook after round 1 (item 113 (7)). **S1-26, riding
with it (not play-shaping):** `DETERMINISM_TICKS` becomes one full first-round Push, 3 600 ticks (3
min at 20 Hz), on `DETERMINISM_MATCH_SEED` unchanged (a new seed buys no coverage the scenarios
lack), with every seat sealing a committed copy of the Hold & Build template's instantiation under
`tests/golden/determinism/`, so a later template edit does not move the chain; the segment list, the
harness playbook and the fifty walkers per seat go. *Downside:* the determinism step runs three
times the ticks on every leg, and G3′'s walker load leaves the chain (S2's G3′-real measures load).

**17. S1-44, `no_arithmetic.rs`.** *(`ui`.)* **(Recommended):** widen the scanner to arithmetic
methods and name `rig.rs`'s keep-alive sum in AGENTS.md §3 rule 4's list (the main session's
sentence).

**18. [play] How the demo shows a deficit.** *(`grid`, `demo`.)* The Quartermaster holds fabrication
at headroom and a placed beacon is net zero, so S1 has no deficit by itself (item 113 (5)).
**(Recommended):** recycle, on site, the beacon a Generator is homed to (its supply counts only
while homed to a lit beacon); a committed scenario does the same. *Downside:* a deliberate act, not
a pressure the game creates until S2's combat.

**19. F3's fix shape.** *(`fixc`, after reproducing.)* **(Recommended):** a single-instance guard
(two instances also share `last_match.txt` and the match cache). *Alternative:*
`use_custom_user_dir` per instance. *Downside:* two lobbies could still resume one match.
*Amended by item 129 (2) (c):* F3 did not reproduce; the guard waits for F3 to recur in the next
round of testing, and the item closes if it does not.

---

## 8. Deliberately left out

**Out by AGENTS.md §11:** script runtimes, MCP, an SDK or published API, AI seats, manual control,
flags/branch/repeat, grid islands, batteries, beacon capture — unchanged from the skeleton's list.

**Out because a later stage owns it.** **S2:** repair and reclaim drones and salvage, repair
threshold and rebuild destroyed in effect (decision 5), `flee_on_threat` and SAFEST's threat term,
combat, the kill-credit split, craters that lose a vent, the nightly adversarial scenarios,
G3′-real's power budget and table-room totals. **S3:** `toward` (decision 3), `RICHEST` and grade
filters as `FeatureRef` ranks, `in_reach` and enemy-known filters, fog-based liveness and
reachability, withholding the seed from seat tokens (item 127 (13)), aligning `BeaconRef.nearest`
(octile today) with targeting's "nearest" (before the public v0.1), FULL ≤ 50 ms and the size
budget, the forecast's what-ifs, its projected income and projection, `get_map_summary`'s terrain
summary and the fog filter on its feature list (item 130 (3)), `decision_tick_ms`, P6's baseline at
the playtest (item 126 (2) (e)). **S4:** the award fund, blueprints beyond the Generator,
`queue_structure`, the symmetric generator. **S5:** Easy's stepping stones, Normal and Hard, P2's
baseline (item 126 (2) (e)).
**S6:** the lobby's settings screen and Probation's untimed Lulls (item 127 (10)). **v1.1:**
publishing `resolve_refs`, `extract_template` and rebind. **Hardening:** H-05's resumed Lull timer;
U-03, the casual fog policy's missing phase term (no casual match is reachable before S6's settings
screen). **Deferred by targeting.md with no stage named, their numbers reserved:** feature `NEAREST`
in conditions, a risk-weighted rank, a `from` origin override, the non-hashed `target_bound` event,
the fallback latch.

**Out on principle:** a public build before v0.1 (item 127 (4)); a second hash function or a shadow
proto; re-blessing a golden without naming the rule that moved it.

---

## 9. Risks, and the measurement that closes each

| # | Risk | What closes it | Where | If it does not close |
|---|---|---|---|---|
| R1 | `tgt` slips and holds `mine`, `grid`, `oper` and `ui` behind it | The split seam; `tgtw` out of the sim PR; the wave-2 checkpoint at day 15.5 | `tgt` | `mine` takes `seam_choice` from a mapgen-side seam list first and rewrites onto the table later (the code map's fallback) |
| R2 | A new hashed field (bindings, ordinals, the held seam) is missed | §4.8's three places per PR; lens A; save/restore round-trips in each sim PR | `tgt`, `mine` | The cross-OS guard and the resume tests catch it as a bisect |
| R3 | "Nearest" is too slow on a decision tick | P1's counted "nearest" estimates and ns per decision tick | `tgt`, `p1`, `demo` | Its octile pruning; else the owner re-rules nearest's cost |
| R4 | F1 is engine behaviour, not ours | Reproduction by hand before any fix; the injected-click check | `fixc` | Press-to-act on the panel's buttons |
| R5 | The balance check measures Easy's weakness, not the economy | Two matchups and six rounds; the report's per-round income | `check`, `tune` | The owner tunes at the demo by hand, as item 127 (3) accepted |
| R6 | Contract PRs (`con1`, `con2`, `tgtv`, `tgt`, `p1`, `tune`, and `mine` and `build` in part) queue on review | One `xtask` and one proto PR at a time; the week of float | section 4 | The float is spent first, then decision 1's extension grows |
| R7 | The plan-core → verifier cycle blocks the estimate stage | Moving the projection down into the verifier | `proj` | Duplicate the arithmetic with a test that the two agree |

---

*Sources: `docs/spec/pharmakos-spec-v0.6.html` §3, §5–§7, §10, §11, §16, §17; decisions-log §2.7
items 33, 94, 111, 113, 115, 116, 123–127; `docs/design/targeting.md`; `docs/placeholders.md`'s S1
section; `docs/demo/skeleton-demo-record.md`; `docs/design/skeleton-plan.md`; AGENTS.md §3, §4, §5,
§6, §9, §10, §12; `.claude/workflows/wave-lanes.js`; `scripts/merge-train.sh`; the code at
`352e573`.*
