<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# S2, destruction and combat — plan for spec section 17, the wk-29.5 row

**Adopted 2026-10-16 under decisions-log §2.7 item 144 (8).** The main session merges this plan
after two adversarial reviews, under the merge delegation item 141 renewed for S2, and S2's first
wave starts at once. The owner was interviewed on the questions that shape play before the plan was
written (item 144 (1) to (7)); every other recommendation of sections 6 and 7 was put to the owner
in bulk, to stand as its **(Recommended)** option, open to the owner's overrule (item 144 (8), as
item 127 (1) did for S1). The **[play]** marks name the play-shaping decisions, which were put
first; **[risk]** marks a risk the owner accepts or refuses.

**Ruled 2026-10-16 (decisions-log item 144 (11)).** After the plan's two reviews, the owner was
interviewed on decisions 1, 4, 5, 7, 9, 35, 37, 40 with 10, 41 and 43, and on section 4.5's cut
order, and took the plan's recommendation on each except two. On decisions 10 and 40 the owner
chose per-material hardness, against the recommendation: terrain resists a blast by material, as
walls do, by a per-material blast-level row this plan proposes (decision 10 gives the ruled form).
On the cut order, departing from the order this plan wrote and on the main session's recommendation
made after decision 5's answer, the owner ruled that Survey's refresh pass is never cut, so full
Survey ships in S2 whatever happens, two planned cuts remain, and any further cut is the owner's
(section 4.5). To decision 5 the owner added: "I want intelligence and surveillance to be a huge
factor in this game." Every other decision of sections 6 and 7 stands as its **(Recommended)**
option, open to the owner's overrule (the owner's bulk answer, as item 127 (1) had it), and each
decision below says how it was ruled. The interview and the **[play]** marks do not match exactly:
decision 1 was interviewed although not marked, and the other **[play]** decisions passed with the
bulk answer. Decision 32 thereby grants the main session a new authority, separate from item 141's
merge delegation: it takes a planned cut of section 4.5 at that cut's checkpoint. The rulings move
the size to about 126 agent-days and the critical path to about 61 working days, still about 13
plan-weeks; the verification review's fixes, applied the same day, move them to about 128
agent-days and 62.5 working days, about 13.5 plan-weeks (sections 4.1 and 4.3).

**When the bulk ruling falls.** It fell on 2026-10-16, before wave 1, so no lane waits on it. The
plan had it fall after `msrv` merges and before `con3` starts on day 2, with `msrv`, `redate`,
`east` and `base` running before it and `vfy1`, which needs no decision, starting beside `con3`;
each lane's Needs line still names the decisions it builds on, all of them now ruled.

Precedence throughout: `docs/design/decisions-log.md` §2.7 > `docs/spec/pharmakos-spec-v0.6.html` >
`docs/design/co-design-gameplan-api.md` (`docs/design/README.md`); `docs/design/targeting.md` amends
the spec where it says so. Where the spec is silent, nothing is invented here: it becomes an owner
decision with a recommendation and the downside of every option (AGENTS.md §12). Engineering calls
(error handling, types, module layout, test gates, the order of work) are made here and listed in
section 2.3, each "decided; the owner can veto".

**Baseline.** `main` at `7a351e7`, where S1 is closed (items 139 and 143). The skeleton and S1 built
more of the S2 row's machinery than its wording suggests. `crates/sim` already has the 12-phase tick
with a Combat slot before KillCredit (`world.rs` `PHASE_ORDER`), a damage queue whose orders are
applied in filing order with kill credit booked before the hit points drain, the largest-remainder
kill-credit settlement and its hashed counters (`credit.rs`), value following condition, local
elimination (a destroyed beacon's structures turn to ruins and its units re-home), seat elimination,
the commander's growing respawn, the one-tick match end (`LastSeatStanding`, `NoSurvivor`,
`RoundLimit`), the copy-on-write crater (`VoxelStore::crater`), cluster repair and the per-tick
repath cap of 16, a wreck table and a salvage program, and the unit and structure kinds the kit
needs (`RepairDrone`, `Raider`, `Scout`; `Autocannon`, `Mortar`, `Wall`). What is missing is
everything that makes a shot: firing, weapon rows, materials, crater damage, collapse, wrecks on
death, repair, Defend and Attack, full Survey, the threat model, the nightlies and the real G3′
measurement. Section 3 says, lane by lane, where each lands.

---

## 1. Goal and end state

Spec section 17, the wk-29.5 row, verbatim: "Mortar, wall, demolition charge, breach, friendly fire
· repair/reclaim drones · full Survey · commander hunting and bodyguard · kill-credit split ·
local-elimination beacon death · match-end tick rule · the three nightly adversarial scenarios start
· exit: G3′ real measurement (300 units / 40 beacons), which sets the per-map power budget."

§2.7 has since added to it: Build's terraform, repair threshold and rebuild destroyed (item 128 (d),
which keeps `BuildSettings` 6 and 7 reserved until S2); a simple deterministic support rule for
collapse, behind an interface physics replaces later (item 138 (3)), with the owner's three answers
(item 144 (1) to (3)); one sensor interface for spheres, unit vision, pickets and Survey (item 138
(4) (c)), with vision a radius and no line of sight (item 144 (4)); the core's autocannon (item
123); the gateway doors for Defend settings, Attack settings and `beacon_under_attack` (item 104);
terraform's meaning (item 144 (5)); the nightlies against Easy (item 144 (6)); the whole row planned
with a cut order (item 144 (7)); G3′-real as the named depth task (item 144 (8)); and S1's carries
(items 136 (5), 139 (1) rulings 3, 7 and 10, 139 (6), 143 (5)).

### 1.1 The demo the owner reviews

One unsigned Windows zip (the same build on Linux), started as `Pharmakos.exe -- --rounds 6` (item
143 (5)). The lobby looks as it did at S1. The owner starts the lobby's match: two seats, the owner
and Easy, the golden seed, six rounds on the 3 / 5 / 8 ladder, rounds 4 to 6 taking its last entry
(item 40). As at S1, the main session drives the zip by computer use while the owner watches, and
presses Ready to end a Lull rather than waiting it out.

1. **Round 1's Lull.** The wizard offers the S1 templates and two new ones, a Defend template that
   guards the core and the commander, and an Attack template that raids the nearest enemy beacon it
   knows of. The owner seals Hold & Build with three more targets: a short wall across the core's
   approach, an autocannon and a mortar. Hold & Build omits `roe`, which reads as `RETURN_FIRE`
   (decision 4), and a mortar on return fire waits to be hit before it shells anything, so the run
   sheet has the owner set the Build beacon's `roe` to `FREE_FIRE` in the editor before sealing.
   The editor shows the wall's price, `$` 1 a voxel, in the projection, and W0501 is gone from
   `seam_choice: SAFEST`.
2. **Round 1's Push.** With the Build beacon's `terraform` set to `BOTH`, a build drone levels the
   mortar's footprint before it builds: the ground under the target rises or falls to the anchor's
   height, a voxel at a time, and each voxel leaves the treasury at the wall's price. The wall goes
   up voxel by voxel, two voxels high at least, since a one-voxel wall is a step anyone climbs
   (decision 40). The core's own autocannon stands from the first tick.
3. **Round 2.** Easy's raiders come for the nearest structure. The owner's autocannon and the core's
   cut them down; each dead raider leaves a wreck. A raider reaches the mortar and damages it below
   Hold & Build's repair threshold (decision 38). The Build beacon's repair drone walks to the
   mortar and repairs it, paying for the hit points it restores (decision 43), then salvages the
   wrecks, and the feed shows `$` coming in at 25 % of each wreck's build cost.
4. **Round 3.** The owner's mortar, on `FREE_FIRE` since step 1, shells the clump of Easy's raiders
   gathering in its range (decision 41). The shell is slow and visible in flight, and lands where
   its target stood when it was fired, so a raider that kept walking is missed (decision 3). It
   lands as a crater: inside its ball the dirt is gone at once and the stone beneath it cracks,
   opening on the next shell, since terrain resists a blast by material (decision 10). A unit
   standing where the shell lands takes splash damage, the owner's own included, since friendly fire
   is on; the mortar never chooses a landing point whose splash holds an own asset.
5. **Round 4.** The owner's Attack beacon, `breach_allowed`, gathers raiders at its staging point
   and launches on force size. Easy's wall blocks the route, so the breach asks the owner's mortars
   in range to shell the first wall column (wall voxels crack on the first blast and break on the
   next) and sends a raider carrying a demolition charge to the wall's footing; the fuse runs out
   and the charge cuts the wall and the ground under it flat down to the wall's footing, so the
   raiders walk through the gap (decision 40). Nothing falls: a wall is filled down to the ground,
   so the charge's flat cut leaves nothing above it unsupported (decision 39). **The collapse is
   staged, and the run sheet says so.** No geometry play can reach, on the golden seed or any
   other, puts an asset on a region a blast can detach (decision 39 says why), so beside the zip
   the run sheet runs `fall`'s headless `collapse_credits_the_blast`, whose terrain shelf is built
   through test seams, and shows its event log: two of the owner's mortar craters cut the shelf's
   last support, it falls, everything standing on it falls with it and takes the support rule's
   flat damage, and the kill credit goes to the owner, whose craters detached it (item 144 (1),
   (2)).
6. **Round 5.** The owner's Attack beacon targets Easy's commander from its last sighting and
   retargets live while a raider sees it. Easy, whose commander died in round 4's breach, has added
   a Defend guard on it (spec §14; decision 35): its guards escort the commander inside that
   beacon's sphere and intercept. The owner's own commander is hunted in turn by Easy's raid: it
   takes damage, the 20 % reflex walks it to the safest own beacon, which, while it is wounded, is
   one whose sphere holds a live repair drone (decision 6), where the drone heals it, and when it
   dies it respawns at the core after the growing delay. A commander is not an asset, so its death
   credits nobody (spec §4).
7. **A beacon falls.** The owner's mortars destroy one of Easy's beacons. Its mandate ends at once,
   its structures turn to neutral ruins and its walls to neutral walls (decision 42; the sim reads
   them neutral, and until the art pass the client still draws them in Easy's colour, E14), its
   units retreat and re-home to Easy's nearest beacon, and the owner's repair drones salvage the
   ruins inside their own sphere.
8. **The recap** shows, between rounds, the owner's own kills and losses, a "shared kill" where
   another seat's damage helped finish an asset, and the value the owner's seat destroyed; at match
   end it shows every seat's. Between rounds Easy's tally is on no wire (the owner's ruling,
   decisions-log item 145 (7), amending spec §3's fog-policy line: `GetRecapResponse.tally` is the
   calling seat's own, and `seat_tallies` is filled only once the match has ended).
9. **The end.** The match ends on the first tick Easy has no beacon left (`last_seat_standing`), or
   at round 6's limit, where the final audit names the winner and its destroyed-value term is now
   not zero (X-08).

Beside the demo: the G3′-real certifying report (section 5), measured on the owner's machine under
Windows and WSL2 Linux and re-run on `tune2`'s numbers; the nightly report, three arms against Easy
on the fixed seed set, each band written and acknowledged as Easy's (item 144 (6)); and the headless
balance and duel reports from which the owner tunes combat's numbers (item 144 (8)), the balance
report with its three-seat run's share of damage aimed at the trailing seat (decision 36).

### 1.2 The machine-checkable half — AGENTS.md §10, item by item

| §10 item | What closes it at S2 | Task |
|---|---|---|
| 1. `cargo xtask ci` green on the matrix, the OSes agreeing on the chains | Every lane green on the three-OS matrix; the cross-OS guard over every chain, digest and path hash, the new S2 scenarios' chains included | all; `demo2` |
| 2. Hashes committed, every movement explained | The re-bless ledger of section 4.2; each chain-moving PR names the rule and the first tick that moved | `base` (`expand-east-segment`, item 145 (2)), `ids`, `fire`, `site`, `fall`, `dfnd`, `surv`, `atk`, `oprd`, `opra`, `tune2` |
| 3. Golden playbooks round-trip, verify and render identically | The two new templates under `library/` with their `demo_*` verify and render goldens; the new constructs' render and canonical goldens under `tests/golden/plan-core/` and `tests/golden/verifier/`; each nightly arm qualifying under `night`'s per-PR test (the arms carry no golden of their own, E20) | `vfy2`, `oprd`, `opra`, `night`, `arms` |
| 4. Headless scenarios assert on events **and** hashes | Eight new scenarios under `scenarios/s2/` (section 3 names each lane's), beside the nine committed today, each one a playbook reaches on its seed: a committed scenario has no placement key and `gamectl` reaches no host seam, so `fire`'s and `fall`'s staged cases are `crates/sim/tests` integration tests asserting events and hashes, and `fire`'s two scenarios move to `atk`, the first lane that makes them playable | `site`, `dfnd`, `surv`, `atk` |
| 5. The nightly adversarial scenarios | Rusher, Turtle and Hunter against Easy on the fixed seed set, nightly, each with its written alarm band, every band acknowledged as Easy's at S2's close and re-baselined at S5 (item 144 (6)); `NIGHTLY_SCENARIOS_ENABLED` set by the owner (decision 30) | `night`, `arms`, `demo2` |
| 6. Depth task and gate, or the written fallback | G3′-real certified by a recorded run (section 5), or the spec's fallback taken in writing | `g3h`, `g3r`, `demo2` |
| 7. Docs in the same PR | The spec amendments and AGENTS.md sentences of section 4.4, the main session's docs PRs | main session |
| 8. The owner reviews the demo | §1.1, from `docs/demo/s2-run-sheet.md` | `demo2` |

### 1.3 What "ends playable" means here

Every item of the row, of §2.7's additions above, and of the register's S2 rows and carries (section
2), and nothing else; less whatever the written cut order of section 4.5 has moved to S3 by the time
the stage closes, which is at most Attack's waves and terraform's crater refill: Survey's refresh
pass is never cut, on the owner's word (item 144 (11)). Where the spec is silent the plan's readings
are owner decisions (sections 6 and 7), each now ruled (item 144 (11)); the bulk ruling fell on
2026-10-16, before wave 1, so no lane waits on it (the header says how each was ruled, and each
lane's Needs line names the decisions it builds on).

---

## 2. What §2.7 settled for S2, and where each ruling lands

| Ruling | What it says | Task |
|---|---|---|
| 144 (1) | Everything on a detached region falls with it and takes one flat damage; the core's bore is never detachable | `fall`; the bore's geometry `fire` (E31) |
| 144 (2) | Collapse's kill credit goes to the seat whose edit detached the region; an own edit credits nobody. It amends spec §3's "terrain … credits nobody", which the main session's docs PR says; several seats' edits on one tick are decision 37 | `fall` (the seat on the edit: `base` lays out `VoxelEdit`'s seat and fills it on Mine's digs, `fire` on craters, `site` on terraform's and the wall's edits, `atk` on the charge's cut) |
| 144 (3) | Mine never detaches a region carrying any seat's structure | `fall` (a named line in `mining.rs`, and a re-check when Voxels applies the dig) |
| 144 (4) | Unit vision is a radius, no line of sight; line of sight a seam of the one sensor interface | `ids` (the seam), `con3` (the rows) |
| 144 (5) | Terraform: FILL raises a footprint to its anchor's height, DIG lowers it, BOTH both; FILL and BOTH refill craters and trenches in the sphere to the generated surface; each voxel at the wall's cost | `site` |
| 144 (6) | Nightlies play Easy, about twenty seeds, the 65 % band, acknowledged as Easy's at S2's close, re-baselined at S5 | `night`, `arms`, `demo2` |
| 144 (7) | The whole row planned, with a cut order written in advance | section 4.5 |
| 144 (8) | S2 runs as S1 did; G3′-real is the depth task with the spec's fallback; owner rows in bulk; numbers in range, checked headless, tuned at the demo; the plan merged after two reviews, wave 1 at once | section 5; `tune2`; section 4 |
| 144 (11) | The owner's rulings on this plan: every decision of sections 6 and 7 as recommended, except per-material hardness for decisions 10 and 40; Survey's refresh pass never cut, so the cut order has two cuts; decision 32's authority to take them | sections 4.5, 6 and 7; `con3`, `con4`, `fire`, `view`, `atk`, `tune2` (hardness); `surv` (the refresh) |
| 138 (3) | A bounded, incremental, totally ordered support rule; its place in the phase order, its cache invalidation, its allocations, its worst case measured in G3′; its constants as rows | `fall`, `con3`, `g3r`; design in section 3's `fall` |
| 138 (4) (c) | Spheres, unit vision, pickets and Survey under one sensor interface with two outputs | `ids`; `dfnd` (pickets), `surv` (Survey) |
| 128 (d) | Repair threshold, rebuild destroyed and terraform take effect in S2; `BuildSettings` 6 and 7 held until S2 | `site`; decision 38 for the threshold's reading, decision 15 for 6 and 7 |
| 123 | The core's autocannon arrives with combat | `fire` |
| 104 | The gateway doors for Defend settings, Attack settings and `beacon_under_attack` close at S2 | `dfnd`, `atk` (named places in `crates/gateway/tests/methods.rs`) |
| 63, 65, 69, 91 | G3′'s world totals; the 190 kW budget, `kw_per_unit`, `reserve_percent` and `map_ceiling_kw` moving together; the repath cap re-derived once bursts are measured | `g3h`, `g3r`, `tune2` |
| 64, 68 | Do not charge G2's CSR rebuild twice; re-measure process memory growth | `g3h` |
| 139 (1) ruling 3 | `expand_east`'s example moves: a timeout the walk can meet or a nearer first site. **Amended by item 145 (2):** the example and the spec's worked example stay; `expand-east-segment` gets its own playbook with a nearer site | `base` (moved from `east`, E22) |
| 139 (1) ruling 7 | The fielding limits (S1-37) are revisited at G3′-real | `ids` (live counts), `tune2` (totals) |
| 139 (1) ruling 10 | The Build list's settings row is atomic, failing with a new additive `StepFailure` id | `base` |
| 139 (6) | The ruled markers re-dated by a comment-only sweep granted here; `scenarios/` comments; `economy.rs`'s BMI expansion; `rules.proto`'s stale comments with S1-42 and ruling 14's markers; Expand & Mine's wording; S1-01's sweep and `gamectl --operator` at S2's start | `redate`, `east`, `base`, `con3`, `oprd`, `msrv`, `night` |
| 143 (5) | `-- --rounds <n>`, test-only, in a client lane; the line's format unchanged | `rnds` |
| 141 | The merge delegation runs to S2's stage demo; the radar lab starts after S2, beside S3 | section 4.3; decision 33 |
| 135 (2) (d), (f), (g), (j); 136 (5) | `seal_commitment`'s E-code (S2-14); `committed_dollars`' comment and fields 5 and 6 `optional`; the event vocabulary additive; `build`'s "Found, not fixed" to S2's first sim and verifier lanes | `vfy1`, `con3`, every lane adding an event, `base` |
| 13 | Freshness is a plan-time test; the target is pinned at the seal; Survey scouts refresh sightings before the next Lull | `vfy2` (the test), `atk` (the pin), `surv` (the refresh, never cut: item 144 (11)) |

The spec's §19 tuning items for S2 are placed too: core HP and the turret (decision 17, `tune2`);
the engagement radius as a percentage of the sphere (`con4`'s field, `tune2`'s value); the picket
radius (a `con3` row, `defend.picket_radius_voxels`, proposed 6 beyond the sphere edge, decision
17); the spawn travel distance, "about 1.5 early Pushes, stated in raider and commander travel"
(a line of `tune2`'s balance report, from the generator's spawn pair and the two speed rows, never a
gate); reach memory N (`verifier.reach_memory_ms`, already a row, which `tune2` reports against
Survey's refresh interval and the Attack pin, and S3 re-reads with the reach filters); Survey's scan
and refresh rules (`surv`, the refresh never cut); and the kingmaking check, the share of damage
aimed at the trailing seat (decision 36).

### 2.1 The register's rows, each placed

| Row | What | Where it lands |
|---|---|---|
| S1-01 | The MSRV half: `rust-version` and clippy's `msrv` from 1.85 to 1.98 | `msrv`, S2's first PR |
| S1-13 | `BeaconSummary.core`'s stale marker | `con3` |
| S1-34 | Unit sight radius as a row, and every copy | `con3` (rows), `ids` (the sim), `wgw` (the gateway's `sight_key`, `Sight`, `SphereVision`) |
| S1-37 | The fielding totals, revisited at G3′-real | `ids` (live counts), `tune2` (the totals, from `g3r`) |
| S1-42 | `fingerprint.rs`'s doc, `proto.rs`'s two test messages, `rules.proto`'s constant-swap comments | `con3` |
| S1-43 | Uncommented workspace versions checked against the registry | every lane (standing rule) |
| S1-51 | `crates/verifier/src/interface.rs`'s stale docs | `vfy1` |
| S2-01, M-01 | The nightly flags, a seed set, a machine-readable result, `win_rate_percent` an integer | `night` |
| S2-02 | The safe playbook's rescue rule | decision 6; `dfnd` (`safest`'s repair term), `oprd` and `wgw` |
| S2-03, S2-04 | `AVOID_KNOWN_THREATS`; `safest`'s threat term | decision 12; `dfnd` (the model), `atk` (the route cost) |
| S2-05 | `HEADROOM_VOXELS` as a row | E26; `con3` (row), `ids` (reader) |
| S2-06 | `MAX_ROUTE_NODES` | E27; `base` |
| S2-07 | The edit and damage queues | engineering call E5; `base` |
| S2-08 | The structure and wreck table totals | decision 25; `fire`, `ids` (the ruin reserve, E1), `tune2` |
| S2-09 | The broadphase query radius | engineering call E11; `ids` (the padded query), `fire` (the constant deleted) |
| S2-10 | A dropped Build target strands what was paid for | decision 26; `site` |
| S2-11 | What a seat sees of an enemy asset | decision 5; `con4`, `wgw` |
| S2-12 | `get_view`'s delta mode | E28; measured in `wgw` and `view` |
| S2-13 | `DefendSettings`, `AttackSettings` | `con4` (shape), `dfnd`, `atk` (behaviour) |
| S2-14, S2-15 | The verifier E-codes for `seal_commitment`'s refusal and `scout_count` > 255 | `vfy1` |
| S2-16 | The harness walkers and their cap | `tune2`, after `g3r` |
| X-08 | "Unsmoothed net worth" read as the audit score; the destroyed-value term | decision 7; `fire` |

### 2.2 Carries not in the register, each placed

| Carry | Source | Where it lands |
|---|---|---|
| The comment-only re-dating sweep | item 139 (6); AGENTS.md §6 | `redate` (grant written out there); the sim's six with `base`; `balance.rs`' two and `scenarios/README.md`'s one with `east`; `rules.proto` and the generated tree with `con3`; the two JSON notes with `con3`'s rules re-bless |
| `scenarios/s1/unaffordable-deploy.scenario.jsonc`'s stale comment, `scenarios/README.md`'s walker note | item 139 (6) | `east` |
| `economy.rs`'s "Base Maintenance Indemnity" (two places) | item 139 (6) | `base`; `rules.proto:285`'s third copy, unrouted until now, in `con3` |
| The eight-slot carry, the silent drop from a full target table, the zero answers for missing rows, `mandate.rs`'s `backlogged` | item 136 (5) | `base` |
| `initial_rows` in the sim's order; `hash.rs`'s `append_bytes` | item 136 (5) | `vfy1` |
| A test of the `seal_commitment` door | item 136 (5) | `wgw` |
| `oper`'s wire defaults (`int_of`, `text_of`, `bool_of`, `array_of`, `voxel`, `candidates::score`'s `time_ms.max(1)`) | item 136 (5) | `oprd` |
| Whether to narrow the estimate's `NOT_FOUND` drawing, with `editor.rs`'s doc | item 136 (5), which gives it to "the next `crates/client-gdext` lane" | `view`. The next lanes to touch the crate are `con3` and `con4`, through named places (`rig.rs`' meter read, the `ViewEntity` and `BeaconSummary` literals), then `rnds`, which edits `godot/scripts/host_link.gd` and only its pins in `crates/client-gdext/tests/godot_project.rs`; none of them owns the crate's sources, so the first lane that does, `view`, carries it, as item 136 (5) meant |
| `crates/client-gdext/src/rules.rs`' test comment (~283), "the owner's to tune at S1's demo", ruled by item 139 (1) ruling 6 | item 139 (6) | `view` |
| The root `Cargo.toml`'s research NOTE, which named four of the six guarded crates | handoff NEXT 3 | `msrv`: **done by #113**, the NOTE names all six (item 145 (1)) |
| `gamectl`'s `b_01`-as-core texts (`scenario/run.rs`, `seat.rs`, `strings.rs`) | handoff NEXT 3 | `night`: clarify, do not rename (item 145 (4)); the reference fixture's core keeps `b_01` in `gamectl`'s and the verifier's copies, which share a `report_hash` pin. **Done by #119** |
| `crates/bench/tests/harness.rs`'s doc list of restated items | item 133 (5) | `g3h` |
| G4's bar, 10 matches of 9 600 ticks, which "stays with the nightly workflow and S2" (`xtask/src/main.rs` ~426) | item 116 | `night` |
| `ci.yml`'s perf-alarms comment (~986), which names the tick-minus-pathing mean as S2's | item 116 (6) (b) | `g3r` |
| `crates/sim/tests/allocations.rs` without walkers; a moving crowd's allocations | item 137 (2) (j) | `atk` |
| `voxels.rs`'s "Nothing fills the queue yet" (~274), stale since `mine` | this plan's code check | `base` |
| Expand & Mine's "ore seam", where the spec (§9) and the client say "scrap seam" | item 139 (6) | `oprd` |
| `xtask/src/package.rs`'s Godot import isolated (`smoke_env`) | handoff NEXT 3 | **left out**: no S2 lane owns `package.rs` (`night` holds one named place in it, the replay's scenario list, and nothing else); it stays with that file's next owner |
| The register's line (~411) giving `upload.rs`'s K and B to S2's exit, where `crates/mesher/src/upload.rs` (~44–49) says S6's art pass | digest | the main session's register edit with this plan's docs PR: the code is right |
| `crates/verifier/src/limits.rs` (~203) dating the reach window S2/S3, where `rules.proto` (~727) says S3 | digest | `vfy1` rewords `limits.rs` to S3 |
| `playbook.proto`'s `repair_threshold_pct` comment (~1308–1310, "once it falls below this percentage … 0 means never") and the header's "terraform's meaning beyond its enum is still unruled" (~1302), stale since item 144 (5) | the reviews | `con4`, with decision 38 |
| The no-silent-fallback sweep beyond the sim: `try_from(..).unwrap_or(..)` and `.get(..).unwrap_or(..)` fallbacks counted at `7a351e7` in `crates/gateway` (66), `crates/client-gdext` (34), `crates/mesher` (33), `crates/verifier` (12), `crates/gamectl` (10), `crates/plan-core` (6), `crates/bench` (4), `crates/operator` (3) and `crates/proto`'s hand-written sources | the operator's global rules; AGENTS.md §6's sweep rule (item 135) | each crate's next S2 owner, written into its section: `wgw`, `view` (client-gdext and mesher), `vfy1`, `night`, `vfy2`, `g3h`, `oprd`, `con3` (E7) |
| `MineSettings.flee_on_threat` and `BuildSettings.rebuild_destroyed` made `optional`, so an edit row can write false, their presence priced in `interface.rs`; `BuildTarget.wall_box`'s comment, which says a lower box "is a verifier error", made to say that the verifier refuses only a box too short in its own extent and that the height over the ground is the sim's (`site`) | `vfy2` #125; item 146 (4) | the next `proto/**` pull request. Section 4.6 plans none after `con4`, so it lands with whichever comes first: an S2 lane's, or M-14's (owner, S3) |
| `ci.yml`'s DCO message (~390), which still says to fix a written branch with `git rebase --signoff <base>` | #122's review; item 146 (4) | the firewall-runner `.github/**` pull request (section 4.6), brought to AGENTS.md §8's remedy |

### 2.3 Engineering calls, decided here (the owner can veto each)

These are hygiene, types, layout and test gates under the operator's global rules and AGENTS.md §4,
not rules of play. Each is the main session's call. Where one carries a reading of play it says so
and names the decision that puts that reading to the owner.

- **E1. The fielding totals count live assets, and rows are reused under generational ids.** Today
  `fabricate` counts every row a seat ever produced (`world.rs` `fabricated_by`, "dead or alive (a
  row is never freed)"), so under attrition item 63's world totals become a lifetime cap — 100 units
  a seat at three seats, gone after the first 100 deaths — which the no-artificial-caps rule forbids
  and which spec §6 contradicts ("a mandate's fabricator produces to the work in hand, not to a
  count, so there are no counts or caps"). `ids` changes the count to live assets and reuses dead
  rows, under this id design:
  - **The width stays `u32`, and the layout is additive.** `AssetId` is a 4-bit kind tag over a
    28-bit index today (`knowledge.rs` ~130–175), and `UnitId`, `StructureId`, `BeaconId` and
    `WreckId` (`tables.rs`) are bare rows. The 28 bits split into a **row** in the low 12 bits and a
    **generation** in the 16 bits above it: `tag (4) | generation (16) | row (12)`, and the per-kind
    ids carry the same low 28 bits. Every id written before has generation 0 and so decodes as
    itself, which keeps the event vocabulary's rule for a value's bit layout (new bits that every
    older value holds zero; AGENTS.md §5, item 135 (2) (j)) and leaves the wire, the gateway's ids
    and the event subject's width unchanged. The packed order is (tag, generation, row), so "lowest
    id" reads oldest generation first, then lowest row: total and deterministic. A row-major packing
    (row in the high bits, so that lowest id is lowest row) was weighed and not taken, because it
    renumbers every id already written and breaks that rule.
  - **Bounded by type, not by mask.** A `Row` newtype holds 0 to 4 094 and a `Generation` newtype a
    `u16`; `AssetId::tagged` takes a tag, a `Row` and a `Generation` and cannot truncate, and the
    masking constructor (`index & INDEX_MASK`, "truncated to them … a deterministic answer rather
    than a panic") goes. A table whose room, derived at load, would exceed 4 095 rows is refused at
    load with a typed error, so row 4 095 is never a real row; the world totals (300 units, 40
    beacons, item 63) and the wreck room of decision 25 sit an order of magnitude under it.
  - **A sentinel has no row.** `UnitId::NONE` and `BeaconId::NONE` are `u32::MAX` (`tables.rs` ~54,
    ~2220), whose low bits under the new layout would read as generation 65 535 and row 4 095, and
    `AssetId::of_unit(UnitId::NONE)` would mask it into a unit id today. So the sentinels stay in
    storage, where the snapshot's fixed-width rule wants them, and never reach the API: every column
    accessor that can hold one answers `Option<Id>`, `row()` and `generation()` of a sentinel
    answer a typed `IdError::Sentinel`, and every `AssetId::of_*` refuses a sentinel with that error
    instead of masking it. Test: `a_sentinel_id_has_no_row`.
  - **Every row read goes through `row()`, in both patterns.** The 40 sites that read an id's raw
    value as a row (`usize::try_from(x.raw())`, counted at `7a351e7`: `world.rs`, `mining.rs`,
    `power.rs`, `mandate.rs`, `programs.rs`, `interpreter/{cond,exec}.rs`, and the gateway's
    `targeting.rs` ~137 and ~1141) read `id.row().index()` instead. The second pattern is an id
    column held as a bare `u32` and read as a row through `usize::try_from(*id)`, about seventeen
    sites more, among them `structures.homes()` in `structures_built_by` (`world.rs` ~1544), the
    target table's `built` links (`mandate.rs` ~398, ~431) and beacons (`world.rs` ~1879), the
    sim's `targeting.rs` (~268) and the broadphase's items (`tables.rs` ~2026). Those columns become
    columns of typed ids (`BeaconId`, `StructureId`, `UnitId`), whose raw value already carries the
    generation, so the snapshot's bytes keep their width; `ids`' sweep covers both patterns, and the
    sim's `no_silent_fallback_is_left` (E7) gains the second pattern so it cannot return.
    `credit::asset_state` (~191), which reads `id.index()` as a row and would silently book a kill
    against a reused row, reads the row and checks the generation, answering none for a stale one
    (counted, E4).
  - **A reference into a freed row is checked where it is read, never trusted.** A freed row's
    holders are not all findable when it is freed (a target's `built` link, a structure's home, a
    unit's home beacon, the attacker memory, a kill-credit slot, an Attack pin), so nothing relies
    on clearing them: every read of a referenced row compares the reference's generation with the
    row's, and a stale reference answers a typed `StaleRef`, counted under its enumerated reason
    beside E4's drops, which the reader treats as "gone" (a `built` link to a freed structure is a
    target not going up; a stale pin is the after-action's "moved or died"). A row's own columns
    are reset on reuse (below), which is the other half.
  - **When a row is freed.** A dead row is freed only at the end of the Match phase, after
    `settle_beacon_deaths` (which finds the alive-to-dead edge against `beacon_alive`), the
    eliminations and the respawns have run, and after its death has settled (its wreck made, its
    kill credit booked; E2). The lowest free row is reused first and its generation goes up by one;
    a row whose generation would pass `u16::MAX` is never reused again (checked arithmetic, never a
    wrap), counted under its reason. **Commander rows are exempt**: `settle_respawns` respawns a
    commander into its own row, so that row is never freed. **Core rows are exempt too:**
    `power::core_of` (`power.rs` ~800–819) takes a seat's core as its lowest beacon id and relies
    on a dead core's row staying ("Brownout distance is measured from the core **or its former
    site**", spec §5), and `World::beacon_max_hp` (`world.rs` ~1243–1255) and the brownout
    order's `is_core` (`power.rs` ~1130) read it; a core row is generation 0 and never freed, so it
    stays its seat's lowest id under the packed order and its site stays readable. Test:
    `a_dead_core_still_anchors_the_brownout_order`. On reuse every per-row column is reset, each
    named in `ids`' PR and covered by a test that reuses a row after a death: the router's walk
    state and route, the home beacon, the program and its job columns, the weapon cooldown, the
    kill-credit slots, the attacker memory (E12), the retreat state and any mandate-table row that
    names it.
  - **A ruin holds a row of the world's, never a seat's.** Today a seat's structure share counts
    every row it ever built, ruins included, through the home beacon's row, "correct only because a
    row is never freed" (`structures_built_by`, `world.rs` ~1535–1549), so that one seat's churn
    cannot fill the table and refuse another seat's build "for a reason that seat cannot see"
    (`STRUCTURE_TABLE_ROOM`'s doc, ~181). Under reuse a seat's share counts its **live** structures,
    from the owner column, and a ruin, which is neutral, counts against no seat's share: it holds a
    row in a ruin reserve beside the shares, as large as the structure room, so every structure the
    world can stand at once can turn to ruin together. A ruin that finds the reserve full buries the
    oldest unsalvaged ruin first, counted and announced, which is decision 25's ruled rule for
    wrecks applied to ruins, with the same in-world reason (the ash buries them); a ruin's row is
    freed when it is salvaged or destroyed. So a seat's build is refused only when its own live
    share is full, a refusal it can see, and never because of another seat's ruins. That applies an
    owner rule where it reaches rather than inventing one; the owner can veto it with decision 25.
  - **`b_NN` names count placements, which become state.** `BeaconTable::push` derives a beacon's
    ordinal as `count_of(seat)`, the rows the seat holds alive or dead (`tables.rs` ~2465–2479), and
    `row_of_ordinal` resolves alive or dead (~2519); with reuse, ordinals would repeat and a dead
    `b_03` would stop resolving, so `beacon_gone` would turn into `no_target`. `ids` adds a hashed,
    snapshotted per-seat placement counter: a beacon's ordinal is the counter's value when it is
    placed, and `row_of_ordinal` answers alive, gone (the ordinal is below the counter and no live
    row holds it) or never placed, so `beacon_gone` keeps its meaning and no step failure changes.
    The restore check that every seat's ordinals equal its row count (~2794–2804) becomes a check
    that each ordinal is below its seat's counter and unique among the seat's rows. The per-viewer
    `e_NN` names, which count sightings, are unchanged.
  - **Beacons lost.** Because a beacon row no longer stays at zero HP for the whole match, X-08's
    "beacons lost" becomes a per-seat hashed counter.
  - This is determinism code: the hash, the snapshot (`SNAPSHOT_VERSION` 9 → 10) and every golden
    move together in `ids`.
- **E2. One damage latency rule: every damage lands in the tick it is dealt.** This is the
  engineering half of decision 3, which puts the play reading (simultaneous fire) to the owner. The
  code's comment intends "anything filed while [Combat] ran waits for the next tick" (`world.rs`
  `phase_combat`), but no order filed in the sim survives a tick today, and the damage queue is in
  neither the hash nor the snapshot, so an order held across a tick boundary would be a latent
  desync. S2 makes it uniform the other way. Combat becomes: (1) every armed asset chooses its
  target from the post-Movement state, in total order, and fires; (2) shells whose flight ends this
  tick land; (3) every order filed in (1) and (2) is applied, kill credit booked first, as today,
  with the tick's orders against one target sharing the credit proportionally (E32). Fire is
  therefore simultaneous: no seat's shot lands before another's choice. **Damage from the Voxels
  phase** (collapse's fall damage) is applied at the end of that phase by the same apply function,
  and then, still inside Voxels, the same settle function the KillCredit phase runs books its kill
  credit, credits destroyed value and makes its wrecks, so the Match phase, which runs after Voxels,
  sees the death on the tick it happened: the match ends, the beacon's local elimination runs, the
  shared kill and the final audit count it, and a death on a segment's last tick reaches that
  segment's recap rather than the next one's. Settling inside Voxels is a phase-order change
  (determinism code), named in `fall`'s PR; `fire` factors the settle function so it can run twice.
  Nothing the sim files is pending at a tick's end, so the queue is never state. The host's
  `file_damage` and `file_voxel_edit` seams (`crates/gateway/src/host.rs` ~598–609, test seams no
  handler reaches) file between ticks and land at the next Combat and Voxels; `Snapshot::capture`
  stays infallible (the runner captures only at tick boundaries, before a host can file), and the
  host's save path refuses, with a typed error, while either queue holds an order the host filed,
  rather than saving a pending order. `base` builds that refusal in its named place in `host.rs`.
- **E3. Damage and repair are types that cannot hold a wrong sign.** `Hp::damaged_by` heals on a
  negative amount with no cap (`math/quantity.rs` ~59). `base` makes damage a `Damage` newtype that
  holds 1 to `i32::MAX`, the range a hit point can lose, built by a constructor that refuses zero
  and anything above `i32::MAX` with a typed error, and repair a separate `Hp::repaired_by(Repair,
  max)` that stops at the asset's maximum by stated rule. A zero or negative damage order cannot be
  built.
- **E4. Every dropped order is counted under an enumerated reason.** `phase_combat` skips an order
  whose target row does not exist (`world.rs` ~2662), and an order on an already dead target applies
  nothing and books nothing, silently. `base` gives each drop a reason code
  (`DamageDrop::NoSuchRow`, `::StaleGeneration`, `::AlreadyDead`), counted in an unhashed counter
  beside `seams::WorkCounter` and asserted by tests; two shots that kill one target in one tick are
  the normal case for `AlreadyDead`, and the counter makes it visible rather than an error.
- **E5. Queues are sized so they cannot fill, and a full queue is a typed error.** The edit queue
  (64) and the damage queue (256) are guesses (S2-07). `base` sizes both at load from the world's
  table rooms. The damage queue's worst case is a sum and a product: one direct order per armed
  asset, plus, for splash, the shells that can land on one tick (the shell room, E13) times the most
  assets a splash disc can hold (bounded by the unit, structure and beacon rooms together); and, for
  the Voxels phase, one fall-damage order per asset that can stand on or under a falling region (E2,
  `fall`), bounded by the unit, structure, beacon and ruin rooms together. Combat's orders are
  applied before Voxels files any, so the queue is sized to the larger of the two phases' bounds.
  The edit queue's is one edit per live asset plus one crater per landing shell or detonating charge
  (the support rule moves voxels through `set` itself and files no edit). Each bound is computed
  with checked arithmetic, refused at load with a typed error if it overflows, and its bytes are
  reported in G3′. `request_damage` and `request_voxel_edit` return a typed `QueueFull` error,
  propagated, and a test holds that the sizing covers the worst case. Mining's existing back-off on
  a full queue becomes a match on that error, and the host seams answer the typed error instead of a
  `bool` (`base`'s named place in `host.rs`).
- **E6. The broadphase cannot overflow, by construction.** `phase_broadphase` checks the rebuild
  with a `debug_assert!` only (`world.rs` ~2578), so a release build would query an empty grid.
  `base` sizes the grid from the unit table's room at construction and returns a typed error there
  if it does not fit; the per-tick rebuild is then infallible and its `bool` goes.
- **E7. No silent fallback is left in the sim, and each other crate's next owner sweeps its own.**
  The plan's first reading named eight sites; the class is larger. Counted at `7a351e7`,
  `crates/sim/src` holds 259 `try_from(..).unwrap_or(..)` conversions and 189
  `.get(..)…unwrap_or(..)` reads, among them `mapgen.rs`' `unwrap_or(i32::MAX)` for the core's and
  the commander's HP and the power ceiling (~677, ~808, ~987, ~997, ~1008, ~1038, ~1047, ~1644),
  `rules.rs`' for HP and draw (~439, ~447), `pathing/estimate.rs`' (~183), the seat conversions
  (`mapgen.rs` ~799, `world.rs` ~582, ~4496), `survey.rs`' `unwrap_or(0)` (~56), the tick's own
  `self.tick.next().unwrap_or(self.tick)` (`world.rs` ~2545), which would freeze the clock rather
  than say it ran out, and `runner.rs`' `MatchState::new`, which clamps `round_limit` and every
  ladder entry to one ("clamped, not trusted", ~281). `base` sweeps every one, each classified in
  the PR: a value from the rules table is refused at load with a typed error; a conversion that
  cannot fail by construction is rewritten so the failure is unrepresentable (iterating the columns
  together, the `Row` newtype of E1, a typed index); a reachable failure returns a typed error that
  propagates; and a saturation or clamp that is a real game rule stays, with a comment naming the
  rule (AGENTS.md §4.1). `MatchState::new` takes validated `MatchSettings` built through a
  constructor that refuses zero with a typed error (the gateway already refuses a zero round limit,
  `host.rs` ~288). A sim test, `no_silent_fallback_is_left`, scans `crates/sim/src` for the two
  patterns and fails on any outside an allow-list whose every entry carries its reason, so the sweep
  cannot drift back. The values in range are unchanged, so no chain moves for it. Under AGENTS.md
  §6's sweep rule the sweep stops at the sim's edge; the other crates' counts are in section 2.2,
  and each crate's next S2 owner sweeps its own crate the same way, with the same test in that
  crate.
- **E8. The verifier refuses, by its own code, every construct the sim refuses.** Today a playbook
  can pass the verifier and be refused by `Plan::compile` at `submit_plan` as `INVALID_ARGUMENT`
  (`crates/gateway/src/surface/planning.rs` ~46–62), so the editor's verdict and the sim disagree.
  The sim has no refusal table to read: its refusals are about a dozen inline
  `PlanError::NotAtThisStage` arms in `interpreter.rs` (~1254 to ~2001). So **`base` builds the
  table**: a `pub const` list of (construct, stage) in the interpreter, with a sim test that
  compiles a playbook per construct and holds the list and the arms to each other in both
  directions. **`vfy1` builds the E-codes** in parallel, one per construct, in a new E00xx run after
  E0008, keyed by the construct's name. **The cross-check lands with `base`**: its last commit,
  after `vfy1` has merged, adds `crates/verifier/tests/refusals.rs` as a named place (no verifier
  lane is running then; `con4`'s verifier places are other files), which walks the sim's table
  through the verifier's `default-features = false` dependency and fails while any refused construct
  has no E-code. Each sim lane that lifts a refusal lifts the table entry and the verifier's E-code
  in the same PR, through a named place (section 3), so the two never disagree on `main`.
- **E9. `get_beacon`'s prose follows the structured summary.** The prose prints a visible enemy
  beacon's mandate and HP (`crates/gateway/src/surface/knowledge.rs` ~442–462, `strings.rs` ~507)
  while `BeaconSummary` withholds both (~1102–1112). `wgw` makes the prose say only what the summary
  says; when decision 5 rules what a seat sees, both follow the ruling together.
- **E10. One sensor interface.** `ids` adds `sight::Sensor`: every source (a beacon's sphere, a
  unit's or structure's vision radius from its rules row, a picket, a Survey scout) is a sensor, and
  the interface has two outputs, the live view the fog filter reads and the knowledge store's
  sightings and reach (item 138 (4) (c)). Its line-of-sight method is a seam that answers "visible"
  for every point in the radius (item 144 (4)); a later model replaces the method, not its callers.
  Vision queries go through the broadphase grid, padded as E11 says, rather than `survey.rs`'s
  quadratic scan. A sighting's age stays derived from the tick it was taken at. **Dormant sight,
  the owner's ruling of 2026-10-19 (decisions-log item 146 (2)):** a dormant beacon's units and
  structures give no vision; the commander keeps its own; a beacon's sphere still sees while the
  beacon is alive, dormant or not; a structure still going up sees nothing. `ids` #123 built it in
  `World::sensor_at`.
- **E11. The broadphase query radius is a weapon's range, and a query is exact over positions that
  moved.** `BROADPHASE_QUERY_RADIUS_VOXELS` (16) is referenced nowhere (S2-09); `fire` deletes it. A
  query of radius r touches (2⌈r / cell⌉ + 1)² cells, so any range is answered whatever the cell
  edge; the edge stays the tuning row `broadphase.cell_size_voxels`, which item 67 tied to the
  largest query radius and unit density and which `g3r` re-derives. **The grid is stale by one
  Movement:** it is rebuilt in Broadphase, phase 1 of 12, before Movement (`world.rs` ~2576), and
  `Csr::collect_in_radius` buckets by the rebuild's positions, so a unit that walked into range this
  tick would be missed by Combat and by vision. `ids` pads every query's radius by the largest
  distance any locomotion row moves in one tick (read from the rules at load, rounded up to whole
  voxels), takes the grid's answer as candidates, and tests each candidate's squared distance
  against the unit table's current, post-Movement position. The answer is exact, the phase order is
  unchanged and there is no second rebuild; G3′ measures the pad's cost. A second rebuild after
  Movement was weighed and not taken: it charges the CSR rebuild twice a tick, the cost item 64
  warns against counting twice.
- **E12. Fire's total order, and the state it reads.** Armed assets choose and fire in (seat, kind,
  id) order; a target is chosen from the padded broadphase over post-Movement positions (E11), by
  the asset's targeting rule (section 6, decisions 8, 12, 21 and 41), ties by squared distance and
  then lowest id. Spread, where a weapon has any, is drawn from `Stream::Combat` keyed by (tick,
  seat, asset), the stream's first use; no other purpose draws from it (AGENTS.md §4.7).
  `RETURN_FIRE` and retreat read hashed state, which `fire` adds to the three places in its PR: a
  per-asset attacker memory (the last enemy that damaged it, as an id with its generation, and the
  tick), filled from each applied order's dealer and, for a shell, the shell's firer (E13); and a
  per-unit retreat state (retreating or not, and toward which beacon). The dealer travels on the
  order: `DamageOrder` carries a seat (`by`, `world.rs` ~497–511) and no asset, so `base` lays out
  its source, an `Option<AssetId>` with its generation, `None` for every order on `main`, beside
  `VoxelEdit`'s seat and in the same gateway named place (`crates/gateway/tests/support/mod.rs`'s
  literals), so that `fire`, which fills it, breaks no gateway literal while `wgw` owns the gateway.
- **E13. Shells are hashed entities.** A mortar shell in flight is a row in a new shell table
  (target point fixed at fire, landing tick, seat, the firer's id with its generation, weapon), in
  the hash, the snapshot and the goldens in `fire`'s PR. Its room is derived at load from firings,
  not from mortars, because a dead mortar's shells stay in flight while its row is reused (E1): at
  most one shell per mortar row per cooldown, and a rebuilt mortar cannot fire before its build
  completes, so the room is the structure room times (⌈longest flight ÷ the shorter of the cooldown
  and the mortar's build time⌉ + 1); a test fills the table at that worst case, and a shell that
  could not be filed is a typed error, never a silent drop. A shell lands where it was aimed, with
  no lead, so a unit that moves is missed: the spec's "slow arcing shell that can be dodged" is the
  shell's flight time, not a hit roll. That aim is a play reading, which decision 3 puts to the
  owner.
- **E14. Walls know their owner, and every voxel its damage, through its material.** "Your own
  voxels" (spec §9, friendly-fire avoidance) needs a wall voxel's owner, and the owner's
  per-material hardness (decision 10) needs a terrain voxel's damage level. `fire` gives both
  material ids from 9 up, so ownership and damage live in the voxels the chunk digests already hash
  and no new table is needed: walls take one id per seat and level, two levels by the ruling
  (whole, cracked), which is eight ids at the sim's four seats (E19), and no neutral wall id, since
  whose a wall is now is read rather than written (below) and a neutral id would have no writer;
  terrain takes a cracked id for each level a terrain material can hold beyond its first, up to the
  catalogue's maximum of three levels, which is two ids for each of dirt, stone, the three ore
  grades and the three vent grades, sixteen in all, so the owner can tune any of `blast_levels` from
  1 to 3 at the demo without a code change; and **`BEDROCK`** takes one, the material generation
  writes in the bottom `materials.bedrock_layers_voxels` layers of every column (`con3`'s row:
  whole voxels counted up from z = 0, proposed 1, owner at S2's demo, 0 refused at load because the
  support rule needs a floor). The ids are wire values, additive and never withdrawn, which is why
  the catalogue defines only the range the tuning needs and a row outside 1 to 3 is refused at load
  with a typed error. That is 25 ids, 9 to 33, of the 247 a `u8` has free. **Bedrock and the core's
  bore never crack and are never removed:** a bedrock voxel has one level that no blast steps, a
  bore voxel keeps its generated material and is skipped by every blast through the bore's derived
  table (E31), a charge's cut stops above both, and neither is mined or levelled. What each place
  needs:
  - **The sim's catalogue** (`crates/sim/src/voxels.rs`, `Material`): the new constants and
    `Material::ALL`; a level read and a "one blast later" read; `ore_richness` and `vent_richness`
    answering a cracked ore or vent id with its grade, so that Mine, the feature table's derived
    liveness and the bore's power read a cracked seam or vent as itself (AGENTS.md §4.8); a cracked
    voxel is solid, and so is bedrock, so `is_solid`, pathing's tops and the support rule (`fall`)
    are unchanged. Generation writes whole materials only, and bedrock in the bottom layers, which
    changes the bottom layer of every chunk column: the mapgen digest goldens and every chain move
    once for it, in `fire`, which re-blesses every chain anyway and names bedrock as a reason.
  - **The chunk codec** (`crates/proto/src/chunk_rle.rs`): nothing. It carries any byte; a cracked
    voxel among whole ones only splits a run, so a cratered chunk encodes in more runs, which `wgw`'s
    and `view`'s measurements (E28, the crater stream) count.
  - **The wire's table**, `ViewChunk`'s comment in `gateway.proto`: `con4` writes every new id
    there, wall, cracked terrain and bedrock, additively. The gateway's
    `the_wire_material_table_is_the_sims` (`crates/gateway/tests/view.rs` ~687) holds its `TABLE` to
    `Material::ALL`'s length, so `fire`, which grows `Material::ALL` while `wgw` owns the gateway,
    is granted that `TABLE` as a named place.
  - **The client** (`crates/client-gdext/src/view.rs` `palette_of`, ~80) draws a material its table
    does not name as air, so on `main` between `fire`'s merge and `view`'s a cracked stone voxel
    draws as a hole and a wall as nothing; `view` therefore follows `fire` and maps every new id.
  - **The mesher** (`crates/mesher`, `PALETTE`): rows for cracked terrain, bedrock and walls by
    seat and level, art placeholders for S6's pass; its face mask is keyed by (material, light), so
    a cracked voxel splits a quad, which `view`'s crater stream measures.

  Whose a wall is after its beacon dies or its seat is eliminated (decision 42) is **read, not
  written**: a seat's wall voxel counts as that seat's while it lies inside one of the seat's live
  spheres and as neutral otherwise, so a beacon's death rewrites no voxel and nothing waits for the
  next Voxels phase. The reading is the sim's alone. The client may not make it (AGENTS.md §3 rule
  4: deciding whose a wall is from the spheres would be a gameplay decision), so it draws a wall
  voxel by the seat its material names, whole or cracked, which is who built it; a neutral wall
  looks like its builder's until S6's art pass, when, if the owner wants neutrality drawn, the
  gateway marks it in the view. `view` does not start before `fire` merges (section 4.1). Ruins and
  built structures stay entities with footprints.
- **E15. The destroyed-value term, and the recap's tallies, are accumulated per seat.**
  `audit::destroyed_value` is always zero and `credit.rs` says the split is "emitted and not
  accumulated" (~33). `fire` adds a per-seat destroyed-value column, credited at each kill by the
  seat's settled share of the asset's build cost (largest remainder, spec §3), and per-seat kill and
  loss counters, all hashed and snapshotted; the audit reads the first, and the recap's kill and
  loss tally and shared kills read the counters, never the event bus, which holds 512 events between
  drains and drops past that (`events.rs` ~60–83), so a combat burst could silence an event but
  never a tally. Every S2 scenario asserts that no event was dropped (`EventBus::dropped` zero), and
  `g3h` records the peak events per tick at the gate load against the bus's 512 (its marker stays
  the owner's, at hardening). For decision 36's kingmaking check `fire` also adds an unhashed
  counter, beside `seams::WorkCounter`, of the damage each seat dealt to each other seat's assets.
- **E16. Wrecks are made on death.** A dead unit or structure leaves a wreck worth
  `economy.salvage_percent` of its build cost; nothing produces a wreck today (`world.rs` ~226).
  `fire` makes them, in the settle function E2 runs after Combat and after Voxels, into a table
  whose room decision 25 rules; `site`'s drones salvage them. A ruin (a structure on a destroyed
  beacon) stays a structure row turned neutral, as now, held in E1's ruin reserve and counted
  against no seat's share.
- **E17. The event vocabulary grows additively.** Each lane that needs a kind adds it at the end of
  `EventKind` (next free id 38) and each failure at the end of `StepFailure` (next free id 13, 10
  retired), with a new name, never renaming or reusing one (AGENTS.md §5, item 135 (2) (j)). Events
  are derived output and never hashed. A new kind needs its arms in two exhaustive matches in the
  gateway, `event_text` (`crates/gateway/src/strings.rs` ~34) and `audience_of`
  (`crates/gateway/src/surface.rs` ~2026), so a sim lane's branch does not compile, let alone go
  green, until they exist: the arms and the tests that pin them are therefore **always** granted to
  the sim lane that adds the kind, as a named place of a few lines, in the same PR. When a gateway
  lane is running at the same time, the two edit different lines of the same files, and the one that
  merges second takes the other in by its merge of `main` (E33).
- **E18. The G3′ CI job is a loose regression alarm, and the gate is certified on the owner's
  machine.** A `ci` step must be green on any machine that runs it ("green locally means green in
  CI", AGENTS.md §9), and a wall-clock budget is a property of the machine; spike G3′ found that any
  p99 asserted on shared vCPUs "will either flap or be so loose it asserts nothing", and that the
  budget is "set by the slower of the two" first-class OSes (`docs/spikes/G3prime-segment-budget.md`
  §5, step 7). So: **the gate (spec §16's 25 ms p99 and 4× real time) is certified by recorded runs
  on the owner's i7-9800X under both Windows and WSL2 Linux**
  (`C:/Users/PC/ClaudeProject/bin/wsl-ci.sh`), the slower of the two deciding (section 5); **the CI
  job is an alarm against gross regression, not the gate**: `g3r` adds `cargo xtask g3-budget`
  outside the step table, like `package`, and a job `g3 budget (ubuntu-24.04)` that runs it at the
  gate load and fails only when the hosted runner's tick p99 exceeds the whole 50 ms tick or its
  mean exceeds 25 ms (the gate's numbers doubled, so a shared runner's noise does not redden it and
  a regression that doubles the tick does); the Windows and macOS legs publish the same figures as
  notices. Every timing is taken inside `crates/bench`, the walled harness; `xtask` only runs it and
  reads its report. The job follows the docs-only fast path. It is a contract change (`xtask`,
  `.github/workflows/ci.yml`); whether it joins the ruleset's required checks is the owner's
  (decision 31).
- **E19. What the sim supports for G3′'s seat count.** The sim's tables and decision ticks run four
  seats (the determinism harness has seated four on a three-zone map, `mapgen.rs` ~648), but the
  generator places three zones, so a fourth seat is seated with no core, force, money or power, and
  the gateway refuses more than three (`host.rs` `MAX_SEATS`). What to measure is decision 2.
- **E20. The scenario format gains no key for the nightlies.** The arms are ordinary
  `*.scenario.jsonc` files (`xtask/src/scenario.rs`'s `SUFFIX`) under `scenarios/nightly/`; the
  nightly run overrides their map seed from a seed file through `gamectl` flags, and per-PR
  validation is the `scenario` step's format check plus a `gamectl` test that verifies each arm's
  playbook and plays each arm one round on its own seed (section 3, `night`). No arm carries a 48
  000-line golden chain.
- **E21. `--rounds` has no upper bound.** `serve::Config` reads the round limit as a `u32`, the host
  refuses 0 (`host.rs` ~288), and nothing allocates per round ahead of time (each round's sealed
  file and hash file are written as it is played). A bound would be an artificial cap, so the
  gateway adds none and the client adds none of its own; a refused value is the gateway's refusal.
- **E22. `expand-east-segment` gets a nearer first site; the example stays (amended by
  decisions-log item 145 (2)).** Ruling 3 left the choice open. The timeout cannot be what moves:
  `tune`'s report found the walk from either seat's core is 290 s or more at the commander's speed
  (`rules/README.md`, `commander_cost_per_second`), longer than round 1's 180 s Push, so any timeout
  the walk meets draws W0701. Nor can the example's site move: `east` found that no site is legal
  both on the scenario's map (seat 0's core at (358, 24, 36) on seed `0xca5caded`) and on the
  verifier's reference view (its core `b_01` at (80, 11, 55)), whose spheres do not overlap, and
  that the example's site, (96, 11, 62), lies about 262 voxels **west** of seat 0's core, not east
  of it. So `examples/playbooks/expand_east.jsonc`, the spec's worked example and their proto,
  plan-core and verifier goldens stay unchanged, and the scenario gets its own playbook,
  `scenarios/skeleton/expand-east-segment.playbook.jsonc`, the worked example with its first site
  at (378, 24, 36), 20 voxels east of seat 0's core, keeping the 120 s timeout and the example's
  shape; a sim test holds the copy to the example in everything but the site. The 100 s bound
  never decides anything there: every legal site east of the core is under 25 s away. The work is
  `base`'s, which owns `crates/sim/tests/scenario.rs`. Both seats of `expand-east-segment` seal the
  same file, whose site is a fixed voxel, so **seat 1 still cannot reach it**: the scenario asserts
  that seat 0 places its beacon and walks its tail to it, and that seat 1 still fails loudly, its
  move timing out and its deploy refused with `illegal_site`, then taking its fallback, and its
  summary says so. *Downside:* the spec's flagship example still cannot finish its first step on
  the scenario's map, and `balance.rs`' `expand_east` column keeps reporting that.
- **E23. Pathing under destruction stays 2.5D.** The search graph has one node per column
  (`pathing.rs`), so a crater lowers a column and a cave under an unchanged top is invisible to it;
  a step is legal only when the two tops differ by one voxel at most (`pathing/surface.rs` ~298,
  ~313), which is why a wall must be two voxels high and how a charge's cut reopens a route
  (decision 40). Breach is a second cost mode over that graph (section 3, `atk`): a column whose top
  is wall material is passable at a demolition cost for the breach query only. Tunnels are not
  modelled, which Mine's "never undermines" (item 144 (3)) and the absence of tunnelling in v1 make
  safe.
- **E24. The S2 `get_view` entity fields and any new message shapes are the smallest that serve
  their consumers,** as `con2`'s were (item 130), and the two reviews read them as a contract
  decision.
- **E25. An unset `roe` is not an engineering call.** `playbook.proto`'s header reserves any
  addition to its list of omitted settings that read as defaults to an owner decision, so the
  default `roe`, and which chains it moves, is decision 4; `con4` writes the ruled entry into the
  header's list.
- **E26. S2-05: `HEADROOM_VOXELS` becomes `locomotion.headroom_voxels` = 2** (`con3`'s row at
  today's constant, read by `ids`), so no behaviour changes. (Decision 20 before the reviews.)
- **E27. S2-06: `MAX_ROUTE_NODES` = 1 024 stays a constant** sized to the 384 map, a search bound
  that is technically necessary, with an exhausted search a typed `no_path` counted under its own
  reason rather than a silent stop (`base`), re-derived with S4's map. A constant outside the rules
  table is the cost. (Decision 24 before the reviews.)
- **E28. S2-12: `get_view`'s delta mode is decided by measurement.** `wgw` records the bytes of a
  full entity list at the gate load of 300 units and `view` its per-frame cost; a delta mode is
  added as an opt-in request field only if the full list misses the client's per-frame budget, and
  is otherwise S7's. (Decision 27 before the reviews.)
- **E29. A new table hashes nothing while it is empty.** Item 136's block rule covers columns added
  to an existing table; a **new** table would still write its count prefix and move every chain. So
  each new S2 table (the shell table, Defend's, the explored cells, Attack's) is hashed as a block
  after the existing tables that writes nothing at all while it holds no row and, once it holds one,
  writes the table's own tag, its row count and its rows. The tag keeps two non-empty tables of the
  same row shape from hashing alike; the empty case writes nothing, so a chain that never fills the
  table does not move. The snapshot still carries every table, empty or not, and bumps its version.
- **E30. The walled harness's seams, and the load it drives.** `crates/bench` drives the sim only
  from outside, through the gateway's `Surface::step` (`crates/bench/src/decision.rs` ~366), and the
  gateway's `the_match_is_stepped_in_one_module` admits stepping only in `host.rs`, the surface's
  driver and `control.rs` (`crates/gateway/tests/confinement.rs` ~540). Per-phase timing therefore
  needs two seams, both built by `base` while it owns the sim and the gateway has no running lane:
  (1) **the tick in phases.** `World::step` is split into `begin_tick`, `run_phase(Phase)` and
  `end_tick`, which `step` calls in `PHASE_ORDER`, and the host offers a phase stepper that runs one
  phase per call and **hands control back between phases**: nothing in the tick calls into a walled
  crate and there is no callback (AGENTS.md §4.9), the bench timing each call from outside; a test
  holds that stepping a match phase by phase leaves the same hash chain as stepping it whole.
  **A half-finished tick is never exposed:** while the stepper holds a tick open (a phase has run
  and `end_tick` has not), the host's capture, save and filing seams (`file_damage`,
  `file_voxel_edit`) and its whole-tick `step` answer a typed `MidTick` refusal, never finishing
  the tick silently or saving its half, and the stepper is a type-state (`PhasedTick`, which holds
  the host's mutable borrow for the tick's length), so no code holding it can reach those seams at
  all; the refusal covers a `PhasedTick` dropped mid-tick. Test:
  `a_half_finished_tick_cannot_be_captured_saved_or_filed`; (2) **the gate's layout.** Real play
  cannot reach the gate load: `map_ceiling_kw` 190 at `kw_per_unit` 1 powers 190 units, and 40
  beacons by walk-in at the commander's one voxel a second do not fit one segment. So the load
  generator is a **bench rules variant**, built in memory from `rules/rules.v1.json` with named
  overrides (the power ceiling at the gate load; the fielding totals are world constants, not rows,
  `UNIT_TABLE_ROOM` 300 and `BEACON_TABLE_ROOM` 40 in `world.rs` ~153–192, already at the gate's
  300 units and 40 beacons, so the variant does not touch them), its own `rules_hash` printed in
  every report and never shipped, and a **layout seam**,
  `Host::seed_layout`, that places the gate's 40 beacons with their mandates and their starting
  units through the constructors map generation uses, once, in the lobby before the first Lull, so
  the layout is an input like the seed; it is refused outside the lobby with a typed error. A
  gateway confinement-test line holds that no handler or dispatch arm reaches the phase stepper or
  the layout seam, which only `crates/bench` and tests call. The harness walkers take no part (S2-16
  removes them, `tune2`).
- **E31. The core's bore is a derived table.** Item 144 (1) makes the bore undetachable and `fire`
  makes it unbreakable, but it has no geometry today: `power.rs` (~14, ~19) names it only as the
  core's supply. `fire` defines it as the core anchor's footprint columns from the bedrock layer to
  the generated surface, a pure function of the seed, the rules and the occupied seats, generated
  with the map, regenerated by `Snapshot::restore_into`, never hashed per tick, and pinned by its
  own golden (`tests/golden/mapgen/expected.bore.txt`, a new golden format, so a contract path), as
  the feature table is (AGENTS.md §4.8's derived-table rule).
- **E32. One tick's orders against one target share its kill credit.** `book_credit` books each
  order in filing order and clamps it to the hit points still there (`world.rs` ~2692), so under
  simultaneous fire the first-filed seat, the lowest under E12's order, would take all the credit
  for a volley two seats fired together. `fire` books a tick's orders target by target: the hit
  points the tick removed (the orders' sum, clamped to what was there) are split among the dealing
  seats in proportion to their orders' amounts, as integers by largest remainder with ties to the
  lowest seat (spec §3), the owner's own and seat-less damage crediting nobody. The orders are
  sorted in place by (target, filing index), so nothing allocates. Credit code is determinism code,
  named in the PR.
- **E33. Ids, versions and re-blesses are taken in merge order.** Two lanes running at once may both
  add an event kind (`site` and `fall`, `atk` and `surv`), or both re-bless the same goldens (`con3`
  and `vfy1` every `report_hash` golden, and `ids` and `vfy2` again; `vfy2`, `wgw` and `fire` the
  gateway's; `con4` and `night` `gamectl docs`' golden). Ids are never
  pre-chosen: a lane takes the next free `EventKind` and `StepFailure` ids and the next
  `SNAPSHOT_VERSION` on the `main` it merges in, and the lane that merges second renumbers its
  own in its merge of `main` (its tests name kinds, not raw ids) and re-blesses after that merge,
  naming the re-bless in its PR. A lane takes `main` only by a signed merge, never by a rebase or a
  force-push (decisions-log item 145 (3)). Only one lane at a time bumps `SNAPSHOT_VERSION`, because only the
  sim's one author changes the snapshot (`fall` holds no state and `surv` writes a table `dfnd` lays
  out), so the planned values (10 `ids`, 11 `fire`, 12 `site` if its drones add state, then `dfnd`
  and `atk`) hold in plan order. Each such re-bless is budgeted, about a quarter of an agent-day, in
  the totals of section 4.1.

---

## 3. The tasks

Conventions for every task, as in S1: its own worktree (`git worktree add ../pharmakos-<id> -b
<branch> main`), target directory `D:/build/<id>`, **one crate one agent** (AGENTS.md §6), `main`
taken in only by a signed merge (`git merge --no-ff --no-commit FETCH_HEAD`, then `git commit -s`),
never by a rebase or a force-push, and no process killed that the lane did not start (decisions-log
item 145 (3)), conventional commits, `git commit -s` with the co-author line above it, SPDX
headers, `cargo xtask ci` green before the PR opens, and every guessed value a `// PLACEHOLDER:
<what> — <who>, <when>` listed in the PR body. A lane opens its PR and stops; the main session
merges (item 141). "Contract PR: yes" means an AGENTS.md §5 path; the PR body names the paths.
Estimates are agent-days (ad) including iteration to green, not review latency.

**Standing rules carried from S1.** `xtask` and `.github/workflows/**` belong to no lane except the
one named for them, and at most one such PR is open at a time: `msrv` first (its `cargo xtask ci
--fix` pass may rewrite `xtask/`, a workspace member, so it is that PR while it is open; it touches
no workflow), then `night`, `g3h`, `g3r`, `tune2`, in that order, and the main session's
firewall-runner PR (item 142 (3): `claude.yml`'s `runs-on` moved to `ubuntu-24.04-firewall`, the
`.github/**` pull request after the owner's first triggered `claude.yml` run) takes its turn in that
same order, never while another of them is open; on the schedule of section 4.1 the gap between
`night`'s merge and `g3h`'s start is where it fits. At most one `proto/**` PR is open at a time
(`con3`, then `con4`, then `tune2` only if a row moves or retires). Each `→` in section 4.1 is a
separate `wave-lanes.js` run. Each task's Implements line is its lane's `items`. Every task over 8
ad carries a split seam written before it starts. A lane that adds a committed scenario owns its
path constant and its `SCENARIOS` entry in `crates/gamectl/tests/scenarios.rs` and its line in
`the_committed_list_is_the_whole_set` (items 129 and 135 (2) (l)). A lane that changes `library/`,
`rules/`, `packaging/`, `LICENSES/`, `REUSE.toml` or `godot/` is packaged locally by the main
session before it merges (S1's decision 9). A lane that changes `godot/scripts/` runs the headless
watch check by hand with the `client extension` job's command (item 143 (2)). Every lane re-dates,
in the files it owns, any marker the `redate` sweep had to leave out.

**Named places for the sim's refusals and events.** A sim lane that lifts a `NotAtThisStage` refusal
is granted, in the same PR, the sim's refusal table entry, the verifier's E-code and its case for
that construct (E8), and the gateway's matching row in `crates/gateway/tests/methods.rs`'
`mandate_and_rule_gaps()` (~2457), where the S2 rows are (not `two_door_cases()`); that grant holds
only while no running lane owns the verifier or the gateway, and when one does, the sim lane makes
the edit as its last commit after that lane merges, as S1's `build` did with the gateway's
fingerprint caller. A sim lane that adds an `EventKind` or a `StepFailure` is **always** granted
their arms in `crates/gateway/src/strings.rs` (`event_text`) and `crates/gateway/src/surface.rs`
(`audience_of`) and the tests that pin them, because both matches are exhaustive and the sim lane's
branch does not build without them (E17); a gateway lane running at the same time edits other lines
of those files, and whichever merges second merges `main` in (E33).

**Concurrent named places, listed both ways.** Wherever two lanes run in one crate at once, the
owning lane's Owns line names the other lane's places as excepted, and the named-place lane's Owns
line names its places exactly. On the schedule of section 4.1 the pairs are: `base` and `east` in
`crates/gateway` (east's template case in `tests/methods.rs`; base's `host.rs`, the surface's
driver, `tests/support/mod.rs` and `tests/confinement.rs`); `base`, then `ids`, with `con4`'s sim
place (the Defend and Attack arms of `compile_settings_row` in `interpreter.rs` and their refusal
table entries) while `con4` is open; `ids` and `con4` in `crates/gateway` (ids' `targeting.rs` row
reads; con4's `ViewEntity` and `BeaconSummary` literals); `fire` and `wgw` in `crates/gateway`
(fire's `TABLE` in `tests/view.rs` and its event arms, E14 and E17; wgw owns the rest); `site` and
`fall` in `crates/sim`; `atk` and `surv` in `crates/sim`. Lanes that would otherwise overlap are
sequenced instead: `rnds` after `con4` (`con3`'s `godot_project.rs` and `rig.rs` places and
`con4`'s `ViewEntity` literal, all in `crates/client-gdext`, so `rnds` runs after `night` in its
slot), `con4` after `vfy1` (con4's verifier places),
and `g3h` and `view` apart in `xtask` (`view` adds the mesher stream's case to
`crates/mesher/tests/perf_alarm.rs` only, and `g3h`, which holds `xtask` after `view` merges, adds
its `perf_alarms` entry).

**Re-blesses in the merge train (E33).** Where two running lanes re-bless the same goldens, the one
that merges second merges `main` in and re-blesses, its PR naming the re-bless and the lane it followed. **A
`report_hash` re-bless reaches past the verifier's own goldens.** The verifier's version and
`report_hash` are carried by `tests/golden/gateway/demo_*_verify/`,
`tests/golden/gateway/walkthrough/`, `tests/golden/operator/wizard/*/` and
`godot/fixtures/rows_report.json`, so every lane whose change moves `report_hash` (`vfy1`, `con3`,
`ids`, `vfy2`, `wgw`, `fire`, `site`, `dfnd`, `atk`, `tune2`) is granted those as named places for
the re-bless alone, in a crate no running lane owns, and its PR names them; where a running lane
owns one (`wgw` the gateway's), the second to merge re-blesses.

---

### `msrv` — the whole-tree MSRV sweep (S1-01), S2's first pull request

**Merged as #113 (`200823e`), with no chain or golden moved (decisions-log item 145 (1)).**

- **Owns:** the whole tree for one commit pair: `Cargo.toml` (`[workspace.package] rust-version` and
  the research NOTE), `clippy.toml` (`msrv`), and every file `cargo xtask ci --fix` rewrites,
  `xtask/` included, since `xtask` is a workspace member, which makes this lane the one `xtask` PR
  open while it is. **Branch** `build/msrv-1.98`, worktree `../pharmakos-msrv`, target
  `D:/build/msrv`. It runs alone: no other lane holds a crate while it is open (item 129 (2) (b)).
- **Builds:** `rust-version` and `msrv` 1.85 → 1.98, their markers reworded as discharged; then
  `cargo xtask ci --fix` across the workspace for the lints the raised MSRV turns on, in a second
  commit so the mechanical diff reads apart from the setting. The fix pass must leave the generated
  prost tree (`crates/proto/src/generated/`) untouched, because `crates/proto/tests/generated.rs`
  holds it byte for byte to what `buf generate` writes: any change the pass makes there is reverted
  before the commit, and a lint that fires only there is answered at the generator, never by an edit
  (AGENTS.md §8). Any lint the fix pass cannot apply is fixed by hand or, where it would defeat a
  determinism lint, stopped and raised. The root `Cargo.toml`'s research NOTE (~103–106) names all
  six guarded crates (`plan-core`, `verifier`, `operator`, `gateway`, `gamectl`, `bench`;
  `xtask/src/main.rs` `GUARDED_PACKAGES`).
- **Implements:** register S1-01; item 129 (2) (b); handoff NEXT 3's research NOTE.
- **Needs:** this plan merged; no decision. **Acceptance:** `cargo xtask ci` green;
  `crates/proto/tests/generated.rs` green under `PHARMAKOS_REQUIRE_TOOLS=1`; **no chain and no
  golden moves** (a lint fix that moves one is a behaviour change, and the lane stops and says so).
- **Contract PR:** yes (`clippy.toml`, the workspace manifest, `xtask/` if the fix pass edits it).
  **PLACEHOLDERs:** none. **Agent-days:** 0.5–1.

### `redate` — the comment-only re-dating sweep (item 139 (6))

**Merged as #114 (`80e52f3`) (decisions-log item 145 (1)).** Its found items (`rules/README.md`'s
prose to `con3`; the register's S1-49, S1-52, delegated-tuning and "At this demo" texts to the main
session's docs PR) are done.

- **The grant, under AGENTS.md §6 and item 141 (3).** This lane may edit **comments and nothing
  else** in these files: `crates/operator/src/easy.rs` (1, ~121), `crates/bench/src/decision.rs` (1,
  ~83), and `rules/README.md`'s Settled? table (16 `**PLACEHOLDER**` cells), which is documentation
  rather than data and enters no hash. **The change the grant names** (AGENTS.md §6: a marker sweep
  "keeps each marker's what, who and when, and adds or removes none unless the grant names the
  change"): each marker whose "when" names S1's demo or S1's stage demo keeps its what and its who,
  and the change this lane makes is to its when, which becomes the stage that next revisits the
  value, with the ruling that settled it for S1 cited in the marker's text (item 139 (1) rulings 4
  to 9; a value S1 kept, for example, becomes "owner, S2's demo, with `tune2`'s report", naming the
  ruling that kept it). No marker is added or removed, and each keeps the `<what> — <who>, <when>`
  grammar. The PR shows `cargo xtask placeholders` before and after, lists every file it touched,
  and the main session checks at the merge, reading by hand any line `git diff -U0` shows outside a
  whole-line comment, that only comments changed.
- **Not covered, and why:** `proto/gp/v1/rules.proto`'s 47 markers and their generated copies
  (`crates/proto/src/generated/gp/v1/gp.v1.rs`) go with `con3`, the next `proto/**` PR, since a
  `.proto` comment regenerates the prost tree; `rules/rules.v1.json`'s `note` (line 3) and
  `tests/golden/proto/expected.rules.v1.json`'s copy are JSON data, not comments — re-dating them
  moves `rules_hash` and every `report_hash` golden — so they go with `con3`'s rules re-bless;
  `crates/gamectl/tests/balance.rs`' two markers and `scenarios/README.md`'s one go to `east`, which
  owns those files while this lane runs; `crates/sim`'s six (`world.rs` 3, `rules.rs` 2 at ~186 and
  ~191, `audit.rs` 1 at ~34) are left out, because `base`, which owns the sim, launches with this
  lane, so `base`'s brief carries them (AGENTS.md §6); JSONC files and `#[allow]` reasons are never
  covered.
- **Branch** `docs/redate-s1-markers`, worktree `../pharmakos-redate`. **Implements:** item 139 (6);
  item 141 (3). **Needs:** `msrv` merged; no decision. **Acceptance:** `cargo xtask placeholders
  --check` green; the register's before-and-after in the PR body; no golden moves; the main
  session's local package run, because `rules/README.md` ships in both zips
  (`tests/golden/package/expected.{windows,linux}.txt` list `Pharmakos/rules/README.md`).
  **Contract PR:** no (no contract path is in the grant). **Agent-days:** 0.5–1.

### `east` — the `scenarios/` comments (ruling 3's site move went to `base`)

**Merged as #115 (`ab98304`), cut down to its comment work (decisions-log item 145 (2)).** As
planned, this lane was to move `examples/playbooks/expand_east.jsonc`'s first site into seat 0's
sphere on the `expand-east-segment` seed, with every pin of the example re-blessed. It measured the
move and stopped: no site is legal both on the scenario's map and on the verifier's reference view
(E22, as amended). So the example, the spec's worked example and their proto, plan-core and verifier
goldens stay unchanged, and the scenario's own playbook, its chain and its event log went to `base`.

- **Owned:** `scenarios/README.md`, `scenarios/s1/unaffordable-deploy.scenario.jsonc`'s comment, and
  `crates/gamectl/tests/balance.rs`' two markers as a named place. **Branch**
  `fix/scenarios-expand-east`, worktree `../pharmakos-east`, target `D:/build/east`.
- **Built:** `unaffordable-deploy`'s stale header (~18–19, "the `$` projection … not yet built",
  which `proj` built) now names the two FULL warnings the playbook draws on seat 0's own snapshot,
  W0602 and W0701, and says that a warning does not stop a seal; `scenarios/README.md`'s walker note
  (~155–162) names S2-16 instead of S1's demo; `balance.rs`' two markers are re-dated to "owner, at
  S2's demo, with `tune2`'s balance report", citing item 139 (1) ruling 5. No chain or golden moved.
- **Dropped:** `balance.rs`' `expand_east` column is not repointed. It keeps reading the example and
  reporting that the walk does not fit, item 137 (2) (f)'s finding and E22's recorded downside.
- **Found:** `examples/playbooks/expand_east.jsonc` and `scenarios/README.md` call themselves
  "CONTRACT FILE" in their headers.
- **Implements:** item 139 (6). **Contract PR:** no (`scenarios/README.md`'s prose only, its format
  untouched). **Agent-days:** 1–1.5, as planned, most of it the measurement.

### `base` — `crates/sim`: the carries, the fallbacks, the queues and the harness's seams

**Merged as #116 (`b744182`), with `expand-east-segment`'s own playbook added to it (decisions-log
item 145 (1), (2) and (4)).**

- **Owns:** `crates/sim` (except, from `con4`'s start, `con4`'s named place: the Defend and Attack
  arms of `compile_settings_row` in `interpreter.rs` and their refusal-table entries), every golden
  its changes move, the named places of section 3's preamble, and these named places outside the
  sim, granted because no lane owns the gateway while it runs and `east`'s gateway place is another
  file: in `crates/gateway`, `src/host.rs` (`file_damage` and `file_voxel_edit` answering E5's typed
  error, E2's save refusal while a host-filed order is pending, and E30's phase stepper and layout
  seam), the surface's driver in `src/surface.rs` (the stepper's entry), `tests/support/mod.rs`'s
  `DamageOrder` and `VoxelEdit` literals (~467–481), and `tests/confinement.rs`' lines admitting the
  two seams; in `crates/verifier`, the new `tests/refusals.rs` (E8), as its last commit after `vfy1`
  merges; and, from `east` (item 145 (2)), `scenarios/skeleton/expand-east-segment.playbook.jsonc`
  (new), `scenarios/skeleton/expand-east-segment.scenario.jsonc`, `crates/sim/tests/scenario.rs`,
  `tests/golden/scenarios/expand-east-segment/`, `scenarios/README.md`'s harness-door paragraph and
  layout block, and the harness-door doc in `crates/gamectl/src/scenario/run.rs` (~482).
  **Accepted at the merge, outside the grant** (item 145 (4)): `surface.rs`' `saved_match` calling
  `Host::check_savable`, `tests/host_loop.rs`' one line, the plan-core and verifier projection tests
  whose missing-row premise the load-time refusals removed, and `tests/control.rs`' new test.
  **Branch** `feat/sim-s2-base`, worktree `../pharmakos-base`, target `D:/build/base`.
- **Builds:**
  - **Ruling 10.** `set_mandate_settings`' Build list is atomic: a row that cannot write the whole
    list (the seat's target table full part-way through) writes nothing and fails its step with the
    new `StepFailure` `targets_unwritten`, the next free id (13 on today's `main`; E17, E33);
    `replace_build_targets`' carry of a paid structure covers every entry, not the first eight
    (`TARGETS_PER_BEACON_SLOTS`, ~250); `add_bound`'s and `add_area`'s `false` become typed results
    the row propagates (item 136 (5)).
  - **Load-time refusals** of the rows `structure_cost` and `structure_max_hp` answer zero for
    (~1210–1221) and of `mandate.rs`' `backlogged` threshold, so all three readers become
    infallible. The Wall, which has no `StructureKind` row by design (`rules.proto` ~666), stops
    reaching `structure_row` at all: its cost is `wall_cost_per_voxel` per voxel (`fire`, `site`).
  - **E2's landing half** (apply at the end of the phase; the queue never state; the host's save
    refusal), **E3**, **E4**, **E5** (both queues and the host seams' typed answers), **E6**,
    **E7** (the sim's whole sweep and its test), **E8**'s refusal table, its sim test and, last, the
    verifier's cross-check, and **E27** (S2-06).
  - **`VoxelEdit`'s seat and `DamageOrder`'s source**, laid out now: every edit carries the editing
    seat (`Option<SeatId>`) and every damage order its dealing asset (`Option<AssetId>`, E12), so
    that `fire`, which fills both for combat, changes no gateway literal while `wgw` is running (the
    additive shape of A5's routing). This lane fills the seat on Mine's digs (`programs.rs` ~624,
    the mining drone's seat), because item 144 (2) names a dig among the edits whose seat collapse
    credits; `fire` fills it on craters, `site` on terraform's levelling and refill and the wall's
    voxels, `atk` on the charge's cut, and the host's test seams file seat-less edits. The queues
    are not state, so no chain moves.
  - **E30's two seams** for the walled harness: the tick split into phases with a host-side
    stepper that hands control back between phases, a type-state whose open tick refuses capture,
    save, filing and a whole-tick step with `MidTick`, and the layout seam; the gateway test
    `stepping_by_phase_hashes_as_stepping_whole`.
  - **The re-dating sweep's sim half** (item 139 (6); `redate` leaves the sim to this lane): the six
    markers in `world.rs`, `rules.rs` (~186, ~191) and `audit.rs` (~34) that name S1's demo,
    re-dated as `redate`'s grant says, comments only, shown in the register's before and after.
  - **Wording:** `economy.rs`'s two "Base Maintenance Indemnity" (~248, ~266) → "Basic Minimum
    Income"; `voxels.rs`'s "Nothing fills the queue yet" (~274).
  - **`expand-east-segment`'s own playbook** (E22, as amended): the worked example with its first
    site at (378, 24, 36), 20 voxels east of seat 0's core, sealed by both seats; the chain and event
    log re-blessed once, diverging at tick 1, the walk; seat 0's walk, deploy and rows asserted, and
    seat 1's timeout (2401), `illegal_site` (2406), `no_target` (2411) and fallback.
- **Does not:** change what an id is or reuse a row (`ids`); fire anything (`fire`); any mandate
  behaviour beyond ruling 10.
- **Implements:** items 136 (5), 139 (1) ruling 10 and ruling 3 (the scenario's half), 139 (6);
  register S2-06 (kept a constant by E27, its marker re-dated to S4), S2-07 (discharged).
- **Needs:** `msrv` merged; **no decision** (E2's landing half is the same under either answer to
  decision 3, and the rows it once read are `ids`'). **Acceptance:**
  `a_build_list_that_cannot_be_written_whole_writes_nothing` (step fails with `targets_unwritten`);
  `every_paid_target_survives_a_settings_edit` (a ninth target included);
  `a_table_missing_the_economy_block_is_refused`; `a_row_out_of_range_is_refused_at_load`;
  `damage_filed_in_combat_lands_in_the_same_tick`; `a_save_with_a_pending_host_order_is_refused`;
  `a_zero_or_negative_damage_cannot_be_built` (constructor tests, `i32::MAX + 1` included);
  `every_dropped_order_is_counted`; `the_queues_cannot_fill_in_a_legal_world`;
  `a_zero_round_limit_is_refused`; `a_tick_past_the_last_is_an_error`; `no_silent_fallback_is_left`;
  `the_refusal_table_and_the_arms_agree`; `every_construct_the_sim_refuses_has_an_e_code` (the
  verifier's, last); `stepping_by_phase_hashes_as_stepping_whole`;
  `a_half_finished_tick_cannot_be_captured_saved_or_filed`; `a_mine_dig_carries_its_seat`;
  `the_layout_seam_is_refused_outside_the_lobby`; **no chain and no `report_hash` golden moves** (no
  hashed field and no snapshot change; a chain that moves is a behaviour change the PR names, or a
  bug), except `expand-east-segment`'s, whose playbook changed; and `east`'s Acceptance, moved here
  (item 145 (2)): that chain and event log re-blessed with the first diverging tick and the step that
  now succeeds named (`to_site`, for seat 0), seat 1's failure asserted, and
  `the_scenarios_playbook_is_the_example_but_its_site`. `balance.rs`' column is not repointed.
- **Contract PR:** yes (determinism code: the tick loop's per-phase form; the event vocabulary,
  `StepFailure`). **PLACEHOLDERs:** none new. **Agent-days:** 6.25–7.75. **Split seam:** planned
  as ruling 10, the refusals, E3 to E6 and E8's table as commit set 1; E7's sweep as set 2;
  `VoxelEdit`'s seat, `DamageOrder`'s source, E30's seams and the gateway places as set 3; one PR.
  **As landed** (item 145 (4)): the three sets could not each build (`world.rs`, `runner.rs` and the
  gateway's places carry all three, and the step's `Result` breaks the gateway short of set 3), so
  the PR is three building commits (the scenario's playbook; the hand-written behaviour; E7's
  mechanical sweep), then E8's verifier cross-check and the verification review's fix. E7 found
  about 552 sites, not about 450 (R14), and carried 216, in six files, to `ids`.

### `ids` — `crates/sim`: generational ids, live fielding and the sensor seam

**Merged as #123 (`75583bd`, head `cad71df`) (decisions-log item 146 (1) to (3)).** Every chain
moved once, from its first recorded tick, by E1's new hashed state; the fix pass moved four more
(`against-easy` and `against-easy-three-rounds` from tick 1084, `deploy-and-visit` from 946,
`mine-to-depth` from 2530) by the accumulator reset alone. It built the owner's dormant sight
(E10), swept E7's 216 carried sites to zero, and its `Host::resume` edit was accepted beyond its
literal grant.

- **Owns:** `crates/sim` (except `con4`'s named place while `con4` is open), every golden its
  changes move, the preamble's named places, and named places in `crates/gateway`:
  `src/targeting.rs`' two row reads (~137, ~1141) and any other gateway line that reads an id's raw
  value as a row, granted because `con4`'s gateway places are the `ViewEntity` and `BeaconSummary`
  literals and no gateway lane runs; and, for the typed mid-tick refusal (decisions-log item 145
  (4)), `Host::begin_push` and `end_recap` in `src/host.rs` and `Surface::begin_push` and
  `end_recap` in `src/surface.rs`. **Branch** `feat/sim-s2-ids`, worktree `../pharmakos-ids`,
  target `D:/build/ids`.
- **Builds:**
  - **E1** in full: the additive id layout, `Row` and `Generation`, the load-time refusal of a room
    over 4 095, the sentinels kept out of the API (`Option` accessors, `row()` and every `of_*`
    refusing them), every row read through `row()` in both patterns, the bare-`u32` id columns made
    typed, every reference read checked by generation (`StaleRef`, counted), `credit::asset_state`'s
    generation check, rows freed at the end of the Match phase with commander and core rows exempt
    and every per-row column reset on reuse, the structure share counted over live structures with
    ruins in their reserve, the per-seat placement counter behind `b_NN` and its restore check, and
    the per-seat beacons-lost counter; `SNAPSHOT_VERSION` 10.
  - **E10**, the sensor seam, reading vision radii from `con3`'s rows; `UNIT_VISION_RADIUS_VOXELS`
    deleted, S1-34's sim copy discharged; **E11**'s padded query, which every vision query and,
    from `fire`, every weapon query uses.
  - **E26**'s reader (`HEADROOM_VOXELS` from `con3`'s row).
  - **Wording:** `knowledge.rs`' and `credit.rs`' module docs where E1 changes what a row is.
  - **E7's carried sites** (R14; decisions-log item 145 (5)): the 216 fallback sites `base` left in
    `world.rs` (85), `interpreter/state.rs` (37), `pathing/clusters.rs` (31), `pathing/router.rs`
    (27), `programs.rs` (21) and `tables.rs` (15), each file capped at its count by
    `no_silent_fallback_is_left`, so the counts only fall; the PR says how many remain.
  - **A typed refusal for a transition mid-tick** (decisions-log item 145 (4)): `Runner::begin_push`
    and `Runner::end_recap` answer a typed refusal with a `MidTick` cause, as the filing seams,
    capture and save do, in place of `base`'s bare `false`; `Host::begin_push` and `end_recap`
    (`crates/gateway/src/host.rs`) pass it on, and `Surface::begin_push` and `end_recap` keep
    turning it into the audited `INTERNAL` refusal, all three as named places.
- **Implements:** items 138 (4) (c), 139 (1) ruling 7, 144 (4); register S1-34 (sim), S1-37 (live
  counts), S2-05.
- **Needs:** `base` and `con3` merged; **no decision**. **Acceptance:**
  `a_seat_that_lost_its_units_can_field_again` (attrition past the per-seat share);
  `a_reused_row_does_not_answer_to_its_old_id` (a stale kill-credit slot, a stale sighting and a
  stale event subject miss the new asset); `a_reused_row_starts_with_every_column_reset`;
  `a_commander_row_is_never_freed`; `a_dead_core_still_anchors_the_brownout_order`;
  `a_sentinel_id_has_no_row`; `a_stale_reference_is_refused_and_counted` (a target's `built` link
  and a structure's home into a reused row); `a_ruin_takes_no_seats_share`;
  `a_full_ruin_reserve_buries_the_oldest_ruin`;
  `a_dead_beacon_still_answers_beacon_gone_after_its_row_is_reused`;
  `an_id_written_before_decodes_as_itself`; `a_room_over_4095_rows_is_refused`;
  `beacons_lost_survives_save_and_restore`; `the_placement_counter_survives_save_and_restore`;
  `a_unit_that_walked_into_range_this_tick_is_found`; `vision_reads_its_row`; **every chain**
  re-blessed once (the placement counter and beacons-lost columns move every one), the first
  diverging tick named per chain; every `report_hash` golden re-blessed (the snapshot version).
- **Contract PR:** yes (determinism code: ids, the hash, the snapshot; the event vocabulary, a
  `ruin_buried` kind with its gateway arms, E17). **PLACEHOLDERs:** none new;
  the vision rows' values are `con3`'s; the ruin reserve's size rides `STRUCTURE_TABLE_ROOM`'s
  marker. **Agent-days:** 5.75–6.75 (three-quarters of a day of it for the core rows, the second
  pattern's sites, the sentinels, the stale references and the ruin reserve, from the verification
  review).

### `con3` — S2's rules rows and the next `proto/**` pull request's carries

**Merged as #118 (`ccb4990`) (decisions-log item 145 (1) and (4)).** Granted at the lane, as every
rules-table lane must be: the rules-hash pin `crates/sim/tests/determinism.rs:811`, which moved
`b1f2eef0e75c5ae4` → `8028d41384e6de2f`. Its "found" items are carried in `wgw` and `fire`.

- **Owns:** `proto/gp/v1/rules.proto` and its generated tree, `proto/gp/api/v1/gateway.proto`'s and
  `playbook.proto`'s **comments and the two field changes named below**, `proto/buf.yaml`,
  `rules/rules.v1.json`, `rules/README.md` (after `redate`), the whole of `crates/proto` (its tests
  pin the rules table and the messages S1-42 names), `tests/golden/{proto,docs}`,
  `tests/golden/schema/` if `get_schema`'s answer moves for the two field changes, the re-bless of
  every `report_hash` golden under `tests/golden/verifier` (owned by `vfy1`, which merges first:
  this lane re-blesses after its merge of `main`, E33) and of the gateway's and the operator's that carry it
  (`tests/golden/gateway/demo_*_verify/` and `walkthrough/`, `tests/golden/operator/wizard/*/`,
  `godot/fixtures/rows_report.json`; section 3's preamble), and named places for the restatements
  that hold the rules table equal: `godot/scripts/mesher_rules.gd`'s `RULES_JSON`
  (`crates/client-gdext/tests/godot_project.rs`), the gateway goldens that carry the table's
  `revision`; and, for the forecast's two fields made `optional`, `crates/client-gdext/src/rig.rs`'
  meter read (~845–858, which reads `forecast.bmi_next_dollars` and `forecast.committed_dollars`
  as plain integers behind a presence check) and its test. **Branch** `build/s2-rules-rows`,
  worktree `../pharmakos-con3`, target `D:/build/con3`.
- **Builds:**
  - **Rows.** A row that replaces a constant holds today's constant, with decision 17's proposal in
    its owner PLACEHOLDER for `tune2` to apply, so no chain moves; a row nothing reads yet takes
    decision 17's proposal now. Each names the owner at S2's demo: `locomotion.headroom_voxels` = 2
    (E26); per-kind `vision_radius_voxels` on `UnitKind` and `StructureKind` (S1-34; every kind 8
    today, the scouts' constant, so the sim's answer is unchanged until `ids` reads the rows and
    `tune2` sets them); a `Weapon` message (range, damage, cooldown, splash radius, crater radius,
    shell speed) on the armed kinds — `units.raider`, `structures.autocannon`, `structures.mortar`
    — and on the core (`beacon.core_weapon`); a `Combat` block (the under-attack window for
    Defend's urgency band, the threat-route surcharge, the demolition charge's radius and fuse, the
    repair rate, the terraform rate); a `Defend` block's `picket_radius_voxels` (spec §19); a
    `Collapse` block (`max_region_voxels`, `fall_damage_hp`, `max_visits_per_tick`; item 138 (3),
    decision 11); and a `Materials` block: `blast_levels`, one field per terrain kind — dirt,
    stone, ore and vent, the ore and vent grades sharing their kind's — at decision 10's proposal
    (dirt 1, ore 1, vent 1, stone 2; owner, at S2's demo), which nothing reads until `fire`;
    `wall_min_height_voxels` per decision 40; and `bedrock_layers_voxels`, the bottom layers of
    every column that generation writes as `BEDROCK` (a `uint32` in whole voxels counted up from z =
    0, proposed 1, owner at S2's demo; 0 refused at load, since the support rule needs a floor;
    E14), which nothing reads until `fire`. A wall's two levels are the ruling's and the catalogue's
    (E14), not a row. Field numbers come from the free ranges of each message (`UnitKind` 5+,
    `StructureKind` 4+, `Beacon` 3+, `Locomotion` 13+, `RulesTable` 3–9 and 23+); the lane picks
    them and the reviews read them as a contract decision. `revision` 4 → 5.
  - **The proto carries** (item 139 (6), item 136 (5), handoff NEXT 3): `rules.proto`'s comments on
    `lull_ms` ("time every Lull"), `mining_carry_voxels` and `build_hp_per_second` (no constant left
    to swap); `rules.proto:285`'s "Base Maintenance Indemnity" → "Basic Minimum Income";
    `commander.placement_range_voxels` documented as superseded by walk-in (~463); S1-42's
    `crates/proto/src/fingerprint.rs` module doc and the two `proto.rs` test messages (~1027,
    ~1033); `gateway.proto`'s two "for the owner to confirm" markers (S3-23 ~765, S1-46 ~868, ruled
    by item 139 (1) ruling 14) and `BeaconSummary.core`'s S1-13 marker (the `b_00` rule, ~688);
    `playbook.proto`'s "Refused in S1" on `on` and `covering` (~1002, ~1010); `committed_dollars`'
    comment (an upper bound, absent outside a Lull) and `GetEconomyForecastResponse` fields 5 and 6
    made `optional` (item 135 (2) (f), (g)), with `rig.rs`' meter reading the presence the type now
    carries instead of its own key check; `ResolvedRef.reachable` added (item 133 (3) (i)), which
    `wgw` writes from `resolve_refs` (dropping the `travel_ms` 0 the item calls a magic value) and
    `view` draws. `resolve_refs`' answer is hand-written (`resolved_json`, `surface/planning.rs`
    ~958), not generated from the message, so adding the field writes nothing: until `wgw` adds it
    to `resolved_json` the answer simply lacks it, and no client reads it before `view`, which
    follows `wgw`.
  - **The re-dating `redate` could not do:** `rules.proto`'s 47 markers (the generated copy
    regenerated), and `rules.v1.json`'s `note` with its golden copy, re-dated in the same rules
    re-bless.
  - **E7's sweep** over `crates/proto`'s hand-written sources (the generated tree excepted).
- **Implements:** items 133 (3) (i), 135 (2) (f) (g), 136 (5), 138 (3), 139 (1) ruling 14, 139 (6),
  144 (4), (8), (11) (the blast-level row); register S1-13, S1-34 (rows), S1-42, S2-05; spec §19's
  picket radius.
- **Needs:** `msrv` and `redate` merged (the hold on `east` is lifted: `east` re-blessed nothing,
  item 145 (2)); the ruled decisions 10, 11, 17 and 40.
  **Acceptance:** `buf lint`; `buf breaking` in `WIRE_JSON` mode (making fields 5 and 6 `optional`
  is wire-compatible; any break stops the lane); `the_rules_table_is_in_canonical_form`;
  `the_restated_rows_agree`; every `report_hash` golden re-blessed with the reason "`rules_hash`
  moved: S2's rows added, `revision` 5, the note re-dated", after `vfy1`'s, the gateway's and the
  operator's `report_hash` goldens with them; the gateway goldens that
  carry the forecast's two fields re-blessed if their JSON moves; `rig.rs`' meter test green over a
  forecast without the two fields; `godot_project.rs` green over `mesher_rules.gd`; **no chain
  moves**; the main session's local package run (`rules/` ships).
- **Contract PR:** yes. **PLACEHOLDERs:** every new row's value (owner, S2's demo, with `tune2`'s
  report); the power rows keep item 91's markers (owner, S2's exit, `g3r`). **Agent-days:**
  3.75–4.75 (a quarter-day of it for decision 10's `blast_levels`).

### `con4` — S2's playbook and gateway wire

**Merged as #120 (`2901f95`, head `203730c`) (decisions-log item 146 (1)).** It discharged S2-11
and S2-13 and re-dated M-14 to S3; its carries are in `vfy2`, `site`, `dfnd`, `atk` and `wgw`.

**Granted at the lane** (decisions-log item 145 (4)): the named sim place below, the Defend and
Attack arms of `compile_settings_row`, needed no edit, because `base`'s arms refuse the whole arm.
The constructs that need sim refusals, the base kit and the wall box, live in `compile_build_target`,
so the main session granted, mid-lane, `compile_build_target` in `crates/sim/src/interpreter.rs` with
its `REFUSALS` entries, a new `PlanError::WallBoxOffAWall` with its `Display` arm and helper, and the
doc table; the refusal tests in `crates/sim/tests/interpreter.rs`; and `crates/verifier/src/refusals.rs`.
`crates/gateway` holds no `ViewEntity` or `BeaconSummary` literal (it writes those answers by hand), so
`wgw` fills the new fields there. The lane adds E0019 (the base kit, until `site`) and E0509 (a
`wall_box` on a target that is not a `wall`), and re-dates M-14 to "owner, S3, the next proto PR
after S2's".

- **Owns:** `proto/gp/v1/playbook.proto`, `proto/gp/api/v1/gateway.proto`, `proto/buf.yaml`, the
  whole of `crates/proto` (after `con3`), `tests/golden/{schema,docs,proto}`, and named places for
  the exhaustive matches and whole-struct literals the new fields break: the sim's
  `compile_settings_row` arms for Defend and Attack and their entries in `base`'s refusal table
  (`crates/sim/src/interpreter.rs`; the sim's owner of the day, `base` then `ids`, lists them as
  excepted and does not edit them while this lane is open), the verifier's structure walk for the
  new messages and their E-codes (`crates/verifier/src/structure.rs`, `walk.rs`, after `vfy1`
  merges), `render_plan`'s arms (`crates/plan-core/src/render.rs`), and any `ViewEntity` or
  `BeaconSummary` literal in `crates/gateway` and `crates/client-gdext` (`ids`' gateway places are
  other lines). **Branch** `feat/proto-s2-wire`, worktree `../pharmakos-con4`, target
  `D:/build/con4`.
- **Builds:**
  - **`DefendSettings`** from its reserved 1–15 (S2-13): the protect list (beacons by reference, and
    `my_commander`), the engagement radius as a whole percentage of the sphere radius (1–100),
    pursue (`NEVER`, `SPHERE_EDGE`, `WHILE_IN_REACH`), and the picket's scout count; numbers for
    auto-fortify and unit mix stay reserved (decision 13).
  - **`AttackSettings`** from its reserved 1–15: the target (a beacon reference, a fixed voxel, or
    `enemy_commander`), the retarget rule, the staging point, the launch conditions (force size,
    segment time; the go-code arm reserved for S4), the retreat conditions, the after-action, avoid
    areas, `breach_allowed` and waves, the enum values decision 14 rules.
  - **`BuildTarget`**: the base kit's blueprint ids named in the catalogue comment — `autocannon`,
    `mortar` and `wall` beside `generator` — which amends the comment's "one blueprint until S4"
    (the licensed blueprints stay S4's); a wall target's box (`Area`) per decisions 10 and 40.
  - **`BuildSettings` 6 and 7** stay reserved (decision 15, ruled as recommended);
    **`MandateSettings` 3 (`report_kinds`)** stays reserved (decision 16).
  - **`playbook.proto`'s comments:** the header's list of omitted settings that read as defaults
    gains `roe` as decision 4 rules it (E25); `repair_threshold_pct`'s comment (~1308–1310) states
    decision 38's reading, and the header's "terraform's meaning beyond its enum is still unruled"
    (~1302) gives item 144 (5)'s meaning.
  - **`gateway.proto`**: `ViewEntity`'s fields from its reserved 6–15 that decision 5 rules (HP,
    heading), the new entity kinds (`shell`, `wreck`, `ruin`), `BeaconSummary`'s enemy detail per
    decision 5 from its reserved 7–15, and `GetRecapResponse`'s kill and loss tally, shared kills
    and value destroyed from its reserved range; `ViewChunk`'s material table comment gains the wall
    materials, one per seat and level (no neutral id: neutrality is read, E14), the cracked terrain
    materials, one for each level beyond a terrain material's first up to the catalogue's three, and
    `BEDROCK`, the values `fire` then implements (E14, decisions 10 and 42).
  - **The scoped `buf.yaml` `ignore_only`** for each reserved range discharged, every discharge
    named (AGENTS.md §5).
  - **The new arms are refused until their lanes land:** the sim's compile answers
    `PlanError::NotAtThisStage` naming each construct, with its refusal-table entry, the verifier
    emits that construct's E-code (E8), and `render_plan` names it plainly.
- **Implements:** items 104, 128 (d), 144 (5); register S2-11 (wire), S2-13.
- **Needs:** `con3` and `vfy1` merged; the ruled decisions 4, 5, 10, 13, 14, 15, 16, 38,
  40 and 42. **Acceptance:** `buf lint`; `buf breaking` green with the scoped ignore; the
  reserved-numbers golden moving for exactly the ranges discharged, each named; canonical JSON
  round-trips for every new message; a refusal test per new construct in the sim and the verifier;
  `the_refusal_table_and_the_arms_agree` green with the new entries; `get_schema` and `gamectl docs`
  goldens re-generated; no chain and no `report_hash` moves.
- **Contract PR:** yes. **PLACEHOLDERs:** none. **Agent-days:** 3.5–4.25.

### `rnds` — `crates/client-gdext` + `godot/`: `-- --rounds <n>` (item 143 (5))

**Merged as #124 (`8dc98a0`, head `cd52ecc`) (decisions-log item 146 (1) and (3) (e) and (f)).**
It reads `--rounds=<n>` as well, in the spelling of the readers beside it; its three client
refusals below were accepted at the merge, and AGENTS.md §3 rule 4 names its pieces. Its table in
`godot/scripts/watch_check.gd` (`_rounds_cases`) was a named place granted at its fix pass.

- **Owns:** `godot/scripts/host_link.gd` and its pins in
  `crates/client-gdext/tests/godot_project.rs`. **Branch** `feat/client-rounds-option`, worktree
  `../pharmakos-rnds`, target `D:/build/rnds`.
- **Builds:** `host_link.gd` reads `--rounds <n>` from `OS.get_cmdline_user_args()`, beside its
  `--gamectl=` and `--root=` readers (~98, ~118), and writes the argument's text into the config
  line's round-limit field in place of `ROUND_LIMIT` (~74, ~134–135); absent, the line keeps 3. The
  client parses and bounds nothing: a value that is not a whole number, is 0 or does not fit a `u32`
  is the gateway's refusal (`serve::Config`, `host.rs` ~288), which the lobby draws as it draws any
  host that refuses its config line (E21). The client refuses only the command line's shape and the
  line's framing, never the number, each an enumerated `RoundsRefusal` with its own words, made in
  `start()` before any host is spawned, for a Resume as for a new match (as built by #124; item 146
  (3) (e)): `NO_VALUE`, `--rounds` as the last argument or followed by another option;
  `REPEATED`, `--rounds` given more than once in either spelling; and `BREAKS_LINE`, a value holding
  a tab, a line feed or a carriage return, which would change the line's framing rather than its
  value. The option is not shown in the lobby. The config line's format and `last_match.txt` are unchanged
  (AGENTS.md §5).
- **Implements:** item 143 (5). **Needs:** `con4` merged (`con3` and `con4` hold places in
  `crates/client-gdext`, `con3`'s in this lane's own `godot_project.rs`, so it starts after `night`
  in its slot, on day 11; the hold on `east` is lifted, item 145 (2)); no decision. **Acceptance:** `godot_project.rs` pins the
  reader and the unchanged line format; `watch_check.gd` run by hand with a refused value (`--rounds
  0`) showing the gateway's refusal; the PR names the reader for AGENTS.md §3 rule 4's list
  (section 4.4); the main session's local package run (`godot/` ships). **Contract PR:** no.
  **Agent-days:** 0.5–1.

### `vfy1` — `crates/verifier`: S2's first verifier lane, the carries

**Merged as #117 (`fe33372`) (decisions-log item 145 (1) and (4)).** Beyond this section it added
E0018 (an enum number this build does not name, refused raw), and its edits to
`crates/gateway/tests/{methods,advice,economy}.rs` and the docs golden were accepted at the merge as
tests that pin the verifier's output. Its "found, not fixed" items are carried in `vfy2`, `wgw` and
`dfnd`.

- **Owns:** `crates/verifier`, `tests/golden/verifier`, the named place in
  `godot/fixtures/rows_report.json` if its four goldens move, and, for the `VERIFIER_VERSION`
  re-bless alone, the named places `tests/golden/gateway/demo_*_verify/`,
  `tests/golden/gateway/walkthrough/` and `tests/golden/operator/wizard/*/`, which carry the version
  and `report_hash` (no lane owns the gateway or the operator while this one runs). **Branch**
  `feat/verifier-s2-carries`, worktree `../pharmakos-vfy1`, target `D:/build/vfy1`.
- **Builds:** S2-14, a verifier E-code for a seal whose `$`/`kW` projection overflows the sim's
  types or the forecast's `int32` (what `Surface::seal_commitment` refuses, item 135 (2) (d));
  S2-15, an E-code for `scout_count` > 255 (what `PlanError::ScoutsOutOfRange` refuses); E8's
  E-codes, one per construct the sim refuses today, keyed by the construct's name (the cross-check
  test is `base`'s, after this lane merges); `initial_rows` brought to the sim's order (item 136 (3)
  (r)); `hash.rs`'s `append_bytes` (~151), whose `unwrap_or(u32::MAX)` becomes a checked length with
  a typed error; E7's sweep over the verifier's other fallbacks (12 counted); S1-51's stale docs in
  `interface.rs` (~18, ~366, `count`'s); `limits.rs`' reach window dated S3 (~203);
  `VERIFIER_VERSION` (`lib.rs` ~160) moved from `0.1.0-s1` to `0.1.0-s2`, because it "names the
  stage that produced it" and this build's reports find what S1's could not.
- **Implements:** items 135 (2) (d), 136 (3) (r), (5); register S1-51, S2-14, S2-15. **Needs:**
  `msrv` merged (the hold on `east` is lifted: `east` moved no verifier case, item 145 (2)); no
  decision. **Acceptance:** a
  case for each new code (`every_emitted_code_has_a_case`); the catalogue golden; the verifier's
  `no_silent_fallback_is_left`; **every `report_hash` golden re-blessed**, the gateway's and the
  operator's included, with the reason "`VERIFIER_VERSION` 0.1.0-s2" (`append_bytes`' change should
  move no encoding, since no real length reaches `u32::MAX`, and the PR says so). **Contract PR:**
  no. **Agent-days:** 3.25–4.25.

### `night` — `crates/gamectl`, the nightly harness and its workflow (S2-01, M-01)

**Merged as #119 (`c334ee0`) (decisions-log item 145 (1) and (4)).** Its edits to `xtask/src/main.rs`
and `tests/golden/scenarios/README.md` were accepted at the merge, and its declining of the `b_00`
rename (below) stands.

- **Owns:** `crates/gamectl`, `xtask/src/scenario.rs`, `.github/workflows/nightly-scenarios.yml`,
  the new `scenarios/nightly/` (its README, `seeds.txt` and three placeholder arms that `arms`
  replaces), `scenarios/README.md`'s layout section (after `base`), and one named place in
  `xtask/src/package.rs`, a contract path its PR names: the package's replay step (~478–508),
  which replays every file `scenario::collect` finds, `scenarios/**/*.scenario.jsonc`, and so would
  play the arms, which carry no golden chain, as if they were committed scenarios; it calls the
  played set instead (below). It also re-blesses `tests/golden/docs/reference/expected.docs.txt` for
  its new flags, since `gamectl docs` prints the CLI's usage lines: that golden is `con3`'s and then
  `con4`'s while they are open. **As merged** (decisions-log item 145 (1)): this lane merged after
  `con3` but before `con4`, so it re-blessed the golden after its merge of `con3`, and `con4`, the
  second to merge, re-blesses it again after its merge of `main` (E33). **Branch** `feat/gamectl-nightly`, worktree `../pharmakos-night`, target
  `D:/build/night`.
- **Builds:**
  - **`gamectl scenario run <file> [--operator easy] [--seeds <file>] [--format json] [--out
    <path>]`**, in `cli.rs`'s `OPTIONS`, each refused outside `scenario run` as `--part` and
    `--depth` are. `--operator` names the built-in operator a `builtin` seat plays; Easy, which
    carries the Balanced weighting (spec §14), is the only value, and any other name is a usage
    error naming the values (item 144 (6)). `--seeds` plays the file once per seed, each seed
    replacing `map.seed`; with it, the file's assertions are not checked and the run writes a result
    instead. `--format json` writes the result as one JSON document; `--out` names its file
    (otherwise stdout). Without `--seeds` the command is unchanged.
  - **The seed file:** one `0x`-prefixed 64-bit hex seed per line, `#` comments, blank lines
    skipped; the decoder caps the file's size, the line length and the seed count (the global rule
    for untrusted input), refuses a duplicate seed and a malformed line with its line number, and a
    property test holds that a crafted file only ever returns the typed error.
  - **The seat layout:** a two-seat arm file names one `playbook` seat (the arm) and one `builtin`
    seat; the run alternates them by the seed's position in the file, so even positions put the arm
    on seat 0 and odd on seat 1 (decision 29).
  - **The result:** per seed the seed, the arm's seat, the winners (seat ids, more than one on a
    shared win), the end reason (`last_seat_standing`, `no_survivor`, `round_limit`), the end tick
    and the rounds played, read from the sim's `MatchOutcome` and `audit::final_audit`; and a
    summary with `seeds`, `arm_wins` (the arm the sole winner), `shared`, `losses` and
    `win_rate_percent`, an integer, `arm_wins × 100 / seeds` rounded down (M-01). A shared win is
    not a win for the alarm band (decision 29).
  - **The workflow:** the run step calls `scenarios/nightly/<arm>.scenario.jsonc` (today it names
    `scenarios/<arm>.jsonc`, the wrong suffix) with `--operator easy` (today `balanced`), the seed
    file `scenarios/nightly/seeds.txt`, `--format json` and `--out`; the alarm-band step reads the
    integer and fails nothing, warning above 65 % (spec §15). `NIGHTLY_SCENARIOS_ENABLED` stays
    unset: it is a repository variable only the owner sets (decision 30). A third job runs G4's bar,
    ten seeded matches of 9 600 ticks on each of the three OSes, and a compare job byte-compares the
    three legs' chains, which `xtask/src/main.rs` (~426) left to the nightly and S2.
  - **Per-PR validation of the arm files (E20):** the `scenario` step already validates every
    committed `*.scenario.jsonc`; `xtask/src/scenario.rs` learns that a file under
    `scenarios/nightly/` carries no `hash_chain_equals` and is validated but not played against a
    golden, through a `scenario::collect_played` that leaves `scenarios/nightly/` out, which the
    `scenario` step and the package's replay both call, so `cargo xtask package` never replays an
    arm; a `gamectl` test verifies each arm's playbook (it must qualify) and plays each arm one
    round on its own seed to the round's end with no refusal.
  - **Carried:** `gamectl`'s `b_01`-as-core texts (`scenario/run.rs` ~484, `seat.rs` ~27,
    `strings.rs` ~293) **clarified, not renamed** (decisions-log item 145 (4)): they describe the
    reference fixture, whose core keeps `b_01` in `gamectl`'s and the verifier's copies, which share
    a `report_hash` pin, so they now say which id is the fixture's and that a live core is `b_00`;
    E7's sweep over `crates/gamectl` (10 counted).
- **Implements:** item 116 (6) (j); item 144 (6); register S2-01, M-01. **Needs:** `msrv` merged
  (one `xtask`/`.github` PR at a time: none other is open; the hold on `east` is lifted, item 145
  (2)); the ruled decisions 18 and 29. **Acceptance:** CLI tests for every
  flag, its refusal elsewhere and an unknown operator; the seed decoder's property test; a result
  golden for a two-seed run of a placeholder arm (a new golden format, so a contract path);
  `win_rate_percent` rounding pinned; `collect_played` leaves the arms out, and the main session's
  local package run shows the replay unchanged; the workflow run once through its `force` input on
  the branch and its artifact attached to the PR; the G4 job's compare green on that run;
  `no_silent_fallback_is_left` for `gamectl`. **Contract PR:** yes (`xtask`'s scenario step,
  `xtask/src/package.rs`'s replay list, a workflow, the result golden's format). **Agent-days:**
  4.75–5.75.

### `fire` — `crates/sim`: weapons, shells, materials, wrecks and the kill-credit split

- **Owns:** `crates/sim`, every golden its changes move, the named places of section 3's preamble
  (the `strings.rs` and `surface.rs` arms always, E17), the `TABLE` of
  `the_wire_material_table_is_the_sims` in `crates/gateway/tests/view.rs` as a named place (E14:
  `Material::ALL` grows while `wgw` owns the gateway, and the two edit different files; `wgw`'s
  Owns line lists it as excepted), and the mapgen goldens that bedrock moves. It adds no committed
  scenario: none of its cases is reachable by a playbook when it merges (the new blueprints are
  buildable only from `site`, raiders are fielded only by Defend and Attack, and the scenario format
  has no placement key and `gamectl` no host seam), so its staged cases are `crates/sim/tests`
  integration tests and its two scenarios move to `atk`. **Branch** `feat/sim-combat`, worktree
  `../pharmakos-fire`, target `D:/build/fire`.
- **Builds:**
  - **Firing** (E2's choosing half, E12): the autocannon, the core's built-in autocannon (fed by the
    bore, never shed in a brownout, drawing no `kW`; item 123; spec §5), the mortar and the raider's
    gun, each reading its `Weapon` row; range tests on squared distance over `ids`' padded query
    (AGENTS.md §4.2, E11); cooldowns in game milliseconds as hashed state of the armed assets;
    targets per decisions 8, 21 and 41; `BROADPHASE_QUERY_RADIUS_VOXELS` deleted (S2-09).
  - **The rules of engagement** for every armed asset: `HOLD_FIRE` never fires, `RETURN_FIRE` fires
    only at an asset that damaged one of the seat's own assets inside its range within the
    under-attack window, read from E12's attacker memory, `FREE_FIRE` fires at any enemy in range
    (decision 4 states these readings with the unset case). `retreat_hp_pct` per decision 4, with
    E12's per-unit retreat state. Return fire and retreat are announced (spec §6's Common row): an
    event each.
  - **Shells** (E13, the firer's id carried) and **splash**: a landing shell damages every asset
    within its splash radius, the firer's own included (friendly fire, spec §9), and files a crater
    edit carrying its seat. **Friendly-fire avoidance in aim:** a mortar never chooses a landing
    point whose splash radius holds an own unit, structure, beacon or own wall voxel (spec §9: "the
    built-in mandates aim to avoid your own assets"); a shell that lands on an own asset that walked
    under it still hurts it.
  - **Materials** per decision 10 (per-material hardness, the owner's ruling) and E14: wall
    materials by seat and level, with wall ownership read from the seat's live spheres (decision 42)
    and no neutral id; `BEDROCK`, written by generation in the bottom
    `materials.bedrock_layers_voxels` layers; the cracked terrain materials up to the catalogue's
    three levels, with `ore_richness` and `vent_richness` reading a cracked seam or vent as its
    grade; `materials.blast_levels` read at load, a value outside 1 to 3 refused with a typed error.
    A blast steps every voxel inside its crater radius down one level, wall and terrain alike, and a
    voxel at its last level is removed, so at decision 10's proposal a shell clears dirt, ore and
    vent at once and cracks stone, which the next blast removes; it never removes bedrock or the
    core's bore, whose geometry this lane defines (E31). The demolition charge's cut, which removes
    whatever it holds down to the wall's footing at any level (decision 40), is `atk`'s and calls
    this lane's edit with the cut's shape. A crater that removes a vent's last exposed voxels loses
    the vent, which the feature table's derived liveness already reads (AGENTS.md §4.8). The seat
    on `VoxelEdit`, which `base` laid out, is filled for every crater, and `DamageOrder`'s source
    for every shot, shell and splash (E12).
  - **Deaths:** the settle function factored so that it runs after Combat and after Voxels (E2);
    wrecks into the table decision 25 sizes (E16); E32's proportional credit; the per-seat destroyed
    value and the kill and loss counters (E15), the audit's term and its first tie-break read as
    decision 7 rules (X-08; `audit.rs`' marker discharged); the unhashed seat-against-seat damage
    counter for decision 36; kill-credit settlement exercised at a realistic death rate (G3′ §9.12).
  - **The threat value** of an armed asset, its weapon's damage per second from its row (decision
    12), as a pure function `dfnd` and `atk` read.
  - **Events** (E17): `unit_destroyed`, `structure_destroyed`, `shell_landed`, `wreck_left`,
    `returned_fire`, `retreat_announced`, at the next free ids. No event per shot: shots are counted
    in the unhashed work counter instead, and the recap reads the hashed counters (E15).
  - **Staged integration tests, in place of scenarios** (`crates/sim/tests/combat.rs`):
    `a_mortar_breaks_a_wall` (a mortar, directed at a wall through a test seam, shells it to a gap;
    craters; wall levels; a crater in stone that cracks before it opens) and `a_raid_on_a_mortar`
    (raiders placed through a test seam dive a mortar, the autocannon and the core's kill them,
    wrecks left), each asserting events, the state hash at named ticks and no dropped event. The
    committed scenarios of the same names are `atk`'s, the first lane whose playbooks reach them.
- **Does not:** Build's placement of the new blueprints (`site`), collapse (`fall`), Defend and
  Attack (`dfnd`, `atk`; the breach's choice of a wall is `atk`'s, decision 41). Its tests place
  armed structures and units through test seams.
- **Also (decisions-log item 145 (5)):** the loader of `con3`'s rows refuses at load, with a typed
  error, a zero `cooldown_ms`, a zero projectile `speed_voxels_per_second`, a zero `range_voxels`, a
  blast level outside 1 to 3 and a zero `bedrock_layers_voxels` (`con3` wrote the rows and no
  reader); and this lane defines the counters behind the recap's `Tally` (`con4`'s): a kill, a
  shared kill (a kill whose credit was split with another seat) and a loss (an own asset destroyed,
  whatever destroyed it), with a structure left as a ruin counted among its seat's losses.
- **Implements:** items 17, 123, 144 (1) (the bore), (8), (11) (per-material hardness); spec §3
  (kill credit, the audit), §5 (the core), §9; register S2-08, S2-09, X-08.
- **Needs:** `base`, `ids`, `con3` and `con4` merged; the ruled decisions 3, 4, 7, 8, 10, 12, 21,
  25, 36, 41 and 42. **Acceptance:** `an_autocannon_kills_a_raider_in_range_and_not_out_of_it`;
  `return_fire_waits_for_damage`; `return_fire_answers_a_shell_by_its_firer`;
  `hold_fire_never_fires`; `fire_is_simultaneous` (two assets that kill each other on one tick both
  die); `a_volley_shares_its_kill_credit`; `a_shell_misses_a_unit_that_moved`;
  `splash_hurts_the_firers_own_unit`; `a_mortar_never_aims_where_its_splash_holds_an_own_asset`;
  `a_wall_voxel_breaks_on_its_last_level`; `stone_takes_two_blasts`; `dirt_goes_in_one_blast`;
  `a_cracked_seam_mines_and_a_cracked_vent_feeds_as_itself` (under a test rules variant that gives
  ore and vent two levels); `a_blast_level_outside_the_catalogue_is_refused_at_load`; the gateway's
  `the_wire_material_table_is_the_sims` green over the grown catalogue;
  `a_wall_turns_neutral_when_its_last_sphere_dies`; `bedrock_and_the_bore_survive_any_blast`;
  `the_bore_is_derived_and_pinned` (its mapgen golden);
  `a_crater_that_takes_a_vent_kills_its_feature`;
  `a_death_leaves_a_wreck_worth_the_salvage_percent`;
  `destroyed_value_is_the_settled_share_of_build_cost`; `the_shell_table_survives_save_and_restore`;
  `the_shell_table_cannot_fill`; `the_attacker_memory_survives_save_and_restore`;
  `the_combat_stream_is_drawn_from_nowhere_else`; `a_damage_order_carries_its_source`;
  `generation_writes_bedrock_in_the_bottom_layers`; the two staged integration tests; every chain
  re-blessed (the new per-seat and per-asset columns; the core's cooldown in every match; bedrock in
  the bottom layer of every column), the first diverging tick named (the material levels move
  nothing of their own: nothing blasts in a committed run before this lane's); the mapgen digest
  goldens re-blessed once, for bedrock alone, the bore's golden added; every `report_hash` golden
  (`SNAPSHOT_VERSION` 11).
- **Contract PR:** yes (determinism code: the combat phase, the kill-credit split, the first
  `Stream::Combat` draw, the snapshot, the material catalogue's new wire values and the bedrock
  layer; the event vocabulary; the bore's golden, a new golden format). **PLACEHOLDERs:** none new
  (the levels and the bedrock layer are `con3`'s rows). **Agent-days:** 10.75–13.25, of which
  1–1.5 is decision 10's per-material hardness (the cracked catalogue, the level step for terrain,
  the readers of ore and vent, the load check and their tests); integration tests in place of two
  committed scenarios save a quarter-day. **Split seam:** direct fire, the rules of engagement with
  the attacker memory and retreat state, deaths, the settle function, wrecks, E32 and the counters
  as set 1; shells, splash, materials and their levels, bedrock, wall ownership, craters and the
  bore as set 2; one PR, re-blessed once.

### `wgw` — `crates/gateway`: S2's surfaces

- **Owns:** `crates/gateway` (except, while `fire` is open, `fire`'s named places: the event arms it
  adds, E17, and the `TABLE` of `the_wire_material_table_is_the_sims` in `tests/view.rs`, E14),
  `tests/golden/gateway`, and the byte copies under `godot/fixtures/` its goldens move. **Branch**
  `feat/gateway-combat`, worktree `../pharmakos-wgw`, target `D:/build/wgw`.
- **Builds:** S2-11 per decision 5 in `get_view`'s entities and `BeaconSummary`; E9 (the prose
  follows the summary); S1-34's gateway copies (`surface/watch.rs` `sight_key`, `viewfeed.rs`
  `Sight`, `host.rs` `SphereVision`, ~636) reading the sim's sensor seam's live-view output instead
  of restating spheres; the recap's kill and loss tally, shared kills and value destroyed, read from
  the sim's hashed counters (E15); `ResolvedRef.reachable` written by `resolve_refs` (`con3`'s
  field) into its hand-written answer, `resolved_json` (`surface/planning.rs` ~958), which this lane
  owns, the field otherwise absent from the wire; the `seal_commitment` door's test (item 136 (5));
  `host.rs`'s `SAFE_PLAYBOOK` marker (~103) reworded to decision 6 (the gateway's own fallback, for
  a seat no operator advises, still writes nothing); `verifier_scope`'s sightings for Attack's
  freshness (the seat's own sightings of enemy beacons and of the enemy commander, with their ages;
  the S1 pattern of `tgtw` filling what `tgtv` laid out, the layout `vfy2` set); E28's measurement:
  the bytes of a full entity list at the gate load of 300 units, recorded in the PR; E7's sweep over
  `crates/gateway` (66 counted).
- **Also (decisions-log item 145 (5)):** the silent fallbacks `con3` found on `main`,
  `viewfeed.rs:716` (a chunk that fails to decode drawn as zeros, air) and `schema.rs:242`
  (`i32::MAX` for an index that does not fit), each a typed error; `con4`'s new `ViewEntity` and
  `BeaconSummary` fields (`hp`, `max_hp`, `heading`, `sighting_age_ms`) filled under the fog
  filter; the recap's `tally`, the calling seat's own, and its `seat_tallies`, every seat's, filled
  only once the match has ended and absent before (the owner's ruling, decisions-log item 145 (7);
  `con4` #120 adds the field), with a test that a recap between rounds carries no other seat's; `Money` and `Hp` converted to the
  wire's integers by `try_from`, with a typed error; and the stale comments: `surface/planning.rs`
  (~760) and `surface/knowledge.rs` (~738, ~810), which say a qualifying FULL report may fail to
  price (E0603 ended that; the door stays as defence in depth), and `surface/knowledge.rs` (~1112),
  which says a beacon's HP and a sighting's age stay reserved.
- **Amended by decisions-log item 146 (4), from `vfy2` #125** (already in the lane's brief, recorded
  here): `Scope`'s sightings filled from the seat's own knowledge, each age converted to `u32` with
  a typed error; `vfy2`'s `push_sighting` keeps the freshest of two sightings of one thing, and the
  seat's own commander is never an enemy sighting. The rest of item 145 (5)'s list above stands.
- **Implements:** items 104 (the gateway half), 133 (3) (i), 136 (5); register S1-34 (gateway),
  S2-02 (gateway's marker), S2-11, S2-12 (measured). **Needs:** `con4`, `ids` and `vfy2` merged; the
  ruled decisions 5 and 6. **Acceptance:**
  `an_enemy_beacons_prose_says_what_its_summary_says`; `the_view_shows_what_decision_5_rules` with a
  fogged and a visible enemy; `the_recap_names_a_shared_kill`;
  `the_recap_tally_reads_the_counters_not_the_feed`; `an_unreachable_pick_says_so` (`reachable`
  false, no `travel_ms` 0); `the_seal_commitment_door_refuses_an_unpriced_seal`;
  `no_silent_fallback_is_left` for the gateway; method, walkthrough and view goldens moved and
  explained; the gateway's `report_hash` goldens re-blessed when the scope's sightings land, after
  `vfy2`'s (E33). **Contract PR:** no (a `report_hash` input's content; `vfy2` changes the layout).
  **Agent-days:** 6–7.

### `vfy2` — `crates/verifier` + `crates/plan-core`: S2's constructs

**Merged as #125 (`cb2da2e`, head `7218f86`) (decisions-log item 146 (1), (3) (a) and (b), and
(4)).** No chain moved; as the second of `ids` and `vfy2` to merge, it re-blessed every
`report_hash` golden for snapshot version 10 (E33). The size budget counts `protect` entries, a
duplicate entry is W0502, and its "for later lanes" are carried in `site`, `atk`, `dfnd`, `wgw`,
`vfy3` and section 2.2 (the next `proto/**` pull request).

- **Owns:** `crates/verifier`, `crates/plan-core`, their goldens, the named place in
  `godot/fixtures/rows_report.json`, and, if it changes the verifier goldens' fixture, the named
  place in `crates/bench/src/quick.rs` (item 131 (5)), and, for the scope's re-bless, the
  gateway's and the operator's `report_hash` goldens (`tests/golden/gateway/demo_*_verify/` and
  `walkthrough/`, `tests/golden/operator/wizard/*/`; no gateway or operator lane runs beside it).
  **Branch** `feat/verifier-s2`, worktree
  `../pharmakos-vfy2`, target `D:/build/vfy2`.
- **Builds:** structure and semantic checks for `DefendSettings`, `AttackSettings`, the wall box
  (its height below `wall_min_height_voxels` an error with its own code, decision 40) and the new
  blueprints (ranges, legal places, the go-code arm refused as S4's with its E-code); interface
  pricing for the new fields at decision 15's (S1) one-field-per-element rule (`interface.rs`
  ~417–424 stop answering zero), and the projection's `$` for a wall's voxels, a demolition charge
  and the new structures' `kW` (`projection.rs` ~536–541); Attack's plan-time freshness test (spec
  §6: "Freshness is a qualification test at plan time only; the target is pinned at the seal"):
  `Scope` gains the seat's sightings with their ages (a `report_hash` layout change, the contract
  half of `wgw`'s fill), and an Attack target unknown or older than `verifier.reach_memory_ms`
  (`rules.proto` ~728) is an error with its own code; W0703's emitter (the reach window) where it
  reads the same sightings; `render_plan` prose and canonical form for every new construct; E8's
  codes for the constructs `con4` refuses; an I-level note where a Build beacon's
  `repair_threshold_pct` is 0 (decision 38); decision 4's I-level note for an omitted `roe`, which
  extends `I0003`, the structure stage's note for an omitted setting and the named default it reads
  as (`structure.rs` ~538), to `roe` reading as `RETURN_FIRE`, with its Fix writing the default out;
  the demolition charge's `$` in the projection at decision 9's price; E7's sweep over
  `crates/plan-core` (6 counted). The
  verifier never ranks and never models enemy behaviour (AGENTS.md §3 rule 2): freshness reads ages,
  nothing else.
- **Also (decisions-log item 145 (5)), from `vfy1` and `con4`:**
  - the 65-handler gap, the one reachable sim refusal left with no verifier code
    (`PlanError::TooManyHandlers`), its code reading the sim's `MAX_HANDLERS` rather than writing a
    second number;
  - Defend's and Attack's interface pricing (`interface.rs` names every field now, so it stops
    compiling when another arrives) and the required-field and range checks `con4`'s comments
    state;
  - the wall box's checks: inside the sphere, its `min.z`, a `wall` target with no box (`con4`'s
    E0509 refuses only a box on another target), and its height;
  - the size budget counting `avoid_areas`;
  - render prose for every new construct;
  - the S1 boolean fields an edit row cannot write false (`flee_on_threat`, `rebuild_destroyed`);
  - plan-core's `verify.rs`, which wraps every `Input::new` error as "the rules table is
    incomplete", `InputError::Unhashable` included;
  - the verifier's missing-row branches (`RulesGap::MissingBlock` in `Limits`, `Rates`, `Prices`
    and `Walk`), unreachable since `base`'s load-time refusals.

  **Granted:** `crates/gateway/tests/{methods,advice,economy}.rs` and
  `tests/golden/docs/reference/expected.docs.txt`, which a lane closing a two-door gap or adding a
  catalogue row moves.
- **Implements:** items 13, 104 (the verifier half); spec §6, §11. **Needs:** `con4` and `vfy1`
  merged; the ruled decisions 4, 9, 10, 13, 14, 15, 38 and 40. **Acceptance:** a case per new
  code; an `I0003` case for an omitted `roe`; `full_finds_what_quick_finds`;
  `the_verifier_never_ranks` unchanged; render and canonical goldens for each new construct;
  `no_silent_fallback_is_left` for plan-core; every `report_hash` golden re-blessed (the scope's
  layout), the gateway's and the operator's with them; the `demo_*_verify` and wizard goldens moved
  for the new `I0003` notes on the templates' omitted `roe`, each named. **Contract PR:** yes
  (`report_hash`'s scope encoding). **Agent-days:** 5.5–6.5 (a quarter-day of it for the omitted
  `roe`'s note).

### `site` — `crates/sim`: Build's S2 settings, the base kit's blueprints, repair and salvage

- **Owns:** `crates/sim` except `fall`'s named places (listed in `fall`'s Owns line), every golden
  it moves, the preamble's named places, its scenarios' entries, and, only if section 4.5's cut 2
  is taken at its start, the verifier's refill note with its case and catalogue row and its
  sentence in `rules/README.md` as named places; and `con4` #120's E0019 (its
  `crates/verifier/src/refusals.rs` entry, its case, its catalogue row and their goldens) as named
  places, for the lift below, since no verifier lane runs beside it (decisions-log item 145 (5)).
  **Branch** `feat/sim-build-s2`,
  worktree `../pharmakos-site`, target `D:/build/site`.
- **Builds:**
  - **The base kit's blueprints:** `autocannon`, `mortar` and `wall` as Build targets, the wall a
    box of voxels at least `wall_min_height_voxels` high above the ground in every column, built one
    voxel at a time at `wall_cost_per_voxel` (decisions 10 and 40), the others at their rows. **A
    wall is filled down to the ground:** every column of the box is filled from the ground's top up
    to the box's top, so no wall voxel has air under it (operator item O1; decision 39), and each
    wall voxel's edit carries the building seat.
  - **Terraform** (item 144 (5)): before building, a drone levels the target's footprint to its
    anchor's height — `FILL` raises, `DIG` lowers, `BOTH` both — each voxel at the wall's cost per
    voxel; `FILL` and `BOTH` also refill craters and trenches inside the beacon's sphere back to the
    generated surface, each voxel through `set` (AGENTS.md §4.8); a refill fills air only, so a
    cracked terrain voxel, which is solid, stays cracked (decision 10). The generated surface is a
    derived per-column table, a pure function of the seed and the rules made with the map,
    regenerated by `Snapshot::restore_into` and never hashed per tick, as the feature table is
    (AGENTS.md §4.8), and pinned by a new `tests/golden/mapgen` file (a golden format, so a contract
    path). Refill competes with building for drone time under the mandate's order. Every levelling
    and refill edit carries the beacon's seat (`VoxelEdit`'s seat, which `base` laid out), so a
    `DIG` that detaches a region is credited as any other edit is (decision 37). If section 4.5's
    cut 2 is taken at this lane's start, the refill is dropped from the brief and, since `FILL` and
    `BOTH` still level a footprint and nothing is refused, the lane adds in its place the verifier's
    I-level note on a Build beacon whose `terraform` is `FILL` or `BOTH`, saying that the crater
    refill is from S3, as a named place (no verifier lane runs then), and the same sentence to
    `rules/README.md`.
  - **Repair threshold** (decision 38) and **rebuild destroyed** (item 128 (d)): an own structure
    below `repair_threshold_pct` of its HP is "damaged" for "highest-order affordable target that is
    unbuilt or damaged" and for repair, and 0 means a structure is never repaired; a destroyed
    ordinary target is rebuilt when `rebuild_destroyed` is set; a capability structure never is
    (spec §8).
  - **Repair/reclaim drones** (spec §9): fabricated by Build beacons on work in hand (an own
    structure below the threshold, a wounded own unit or the commander in the sphere, a cracked own
    wall voxel per decision 42, a wreck or ruin in the sphere), they repair those structures,
    **every own unit in the sphere and the commander** at any HP below full (spec §4: "repair drones
    repair it like any other friendly unit"; spec §6's Defend fallback to a repair drone), at the
    repair row and the price decision 43 rules, filed under the Quartermaster's Repair band, then
    salvage wrecks and ruins of any seat inside the sphere for `$`.
  - **S2-10** per decision 26.
  - **Scenarios:** `scenarios/s2/repair-and-salvage` and `scenarios/s2/terraform-refills-a-crater`,
    each reached by playbooks on its seed. Their damage source is the one a playbook can produce
    once this lane merges, a mortar duel: two `playbook` seats each walk the commander to a Build
    beacon near the map's centre in round 1 and build a mortar there in round 2, inside each other's
    40-voxel range, each beacon writing `roe: FREE_FIRE` (decision 4's unset `RETURN_FIRE` would
    wait to be hit). The shells damage the other seat's structures, which its repair drones repair
    and pay for, a destroyed one leaves a wreck or a ruin they salvage, and the craters inside a
    `FILL` beacon's sphere are refilled; a crater in stone cracks before it opens. The core's
    autocannon, the other damage source, fires only at units (decision 8). The lane names in its PR
    the sites it found on the seed; raiders, which only Defend and Attack field, are not needed.
- **E0019 lifted** (decisions-log item 145 (5)): `con4`'s refusal of the base kit's blueprints, the
  sim's `build_target_base_kit` entry and the verifier's E0019, is removed when this lane builds
  them, after `vfy2`; `crates/verifier/tests/refusals.rs` holds the two to each other.
- **The wall's ground-dependent checks** (decisions-log item 146 (4), from `vfy2` #125): the
  verifier makes only the column and box-extent halves of the wall box's checks (E0416, E0417,
  E0511, E0512), because the view carries no terrain. This lane makes the rest in the sim: whether
  the box's `min.z` reaches the ground, the wall's height over the ground in every column against
  `wall_min_height_voxels`, and the 3-D sphere test, each a typed refusal.
- **Implements:** items 128 (d), 144 (5); spec §4, §6's Build row, §7's Repair band, §8, §9;
  register S2-10.
- **Needs:** `fire` merged; the ruled decisions 10, 15, 26, 38, 40, 42 and 43.
  **Acceptance:** `fill_raises_a_footprint_to_its_anchor`; `dig_lowers_it`;
  `fill_refills_a_crater_to_the_generated_surface`; `none_leaves_ground_as_it_is`;
  `each_levelled_voxel_costs_the_walls_price`; `a_wall_lower_than_the_minimum_is_refused`;
  `a_structure_below_its_repair_threshold_is_damaged`; `a_zero_threshold_never_repairs_a_structure`;
  `rebuild_destroyed_rebuilds_an_ordinary_target_only`; `a_repair_drone_heals_the_commander`;
  `a_repair_drone_heals_an_own_unit_in_the_sphere`; `repair_is_paid_for_under_the_repair_band`;
  `a_cracked_own_wall_is_repaired`; `any_seats_wreck_is_salvageable_inside_the_sphere`;
  `a_full_repair_costs_no_more_than_the_build` (from one hit point to full, at a cost and a maximum
  whose quotient rounds every step); `a_wall_box_is_priced_per_voxel`;
  `a_wall_has_no_air_under_it` (on sloped ground); `a_levelling_edit_carries_its_seat`; the two
  scenarios; chains with a Build re-blessed with reasons
  (the determinism chain's Hold & Build writes `terraform` omitted, NONE, and nothing in it is
  damaged, so it should not move: if it does, the PR says why); the next `SNAPSHOT_VERSION` if drone
  jobs add state, every `report_hash` with it.
- **Contract PR:** yes in part (hashed drone state, the event vocabulary, the generated-surface
  golden). **Agent-days:** 7.75–9.25 (a quarter-day of it for the wall filled to the ground, the
  repair price's rounding and the scenarios' mortar duel, from the verification review). **Split
  seam:** blueprints, repair and salvage as set 1; terraform and S2-10 as set 2.

### `fall` — named place in `crates/sim`: the support rule (item 138 (3), item 144 (1) to (3))

- **Owns:** a new `crates/sim/src/support.rs` (which holds its own `impl World` block) and its `mod
  support;` line in `crates/sim/src/lib.rs`; the call lines at the end of `phase_voxels` (the
  support search and the drop, then the settle call of E2); the dig's apply-time re-check lines in
  the Voxels phase's edit application; the dig check's named lines in `crates/sim/src/mining.rs`;
  `VoxelStore::crater`'s return of the voxels it removed (`crates/sim/src/voxels.rs`), which the
  seeds read; its two kinds at the end of `EventKind` in `crates/sim/src/events.rs` and their
  gateway arms (E17); its scratch, one field on `World` (`support: support::Scratch`, unhashed and
  out of the snapshot, so it adds no hash, snapshot or table line) with its construction lines in
  `World::new` and in the restore's construction of the world (`crates/sim/src/world.rs`);
  `crates/sim/tests/support.rs`; a collapse-every-tick case in `crates/sim/tests/allocations.rs`.
  It adds no committed scenario (`collapse-credits-the-blast` needs a geometry no playbook reaches,
  decision 39, so it is the staged integration test below). It runs beside `site`, which lists
  these as excepted (AGENTS.md §6 named place). **Branch** `feat/sim-support-rule`, worktree
  `../pharmakos-fall`, target `D:/build/fall`.
- **The design.** The rule sits behind one interface, `support::Collapse`, with one method the
  Voxels phase calls — given this tick's removed voxels, return the regions that fall and how far —
  so that physics (item 138 (3)'s roadmap entry) replaces the implementation and not its caller.
  - **Support.** A solid voxel is supported when a face-connected path of solid voxels joins it to
    the bedrock layer or to the core's bore (E31), which is never detachable. A region is a
    face-connected set of solid voxels; a region with no such path is detached.
  - **Incremental.** Only voxels that might have lost support are examined: the seeds are the solid
    face-neighbours of every voxel an edit turned to air this tick (craters, `set` to air, digs),
    deduplicated.
  - **Total order and shared labels.** Seeds are sorted by voxel index (the chunk store's linear
    index, chunk-major). Each seed not yet labelled starts a breadth-first search over solid
    face-neighbours, enumerated in a fixed order (−z, +x, −x, +y, −y, +z). One visited set and one
    label per voxel serve the whole tick, so a search that reaches a voxel an earlier search
    labelled stops there with that label, and no voxel is searched twice in a tick. A search ends
    **supported** the moment it reaches a voxel whose own column is solid down to bedrock (a
    per-column "solid from the bottom" height, read from the store and cached for the tick's touched
    columns, which makes ordinary ground next to a crater an O(1) answer), a bore voxel, or a voxel
    already labelled supported. It ends **detached** when its frontier empties. Each detached region
    is identified by its lowest voxel index.
  - **Bounded, per search and per tick.** A search that visits more than
    `collapse.max_region_voxels` voxels without grounding is labelled supported: a mass that large
    is wedged into the ground around it, which is the rule's in-world reason and the technical bound
    (decision 11). The tick's searches together visit at most `collapse.max_visits_per_tick` voxels;
    once that is spent, every seed not yet labelled is labelled supported, counted under an
    enumerated reason (`CollapseSkip::TickBudget`), with nothing pending into the next tick, so the
    rule holds no state. A region the budget did not reach holds for the same in-world reason.
    Without the shared labels and the tick budget the worst case would be the seeds times
    `max_region_voxels` (a radius-4 charge seeds about 200 voxels, so about 800 000 visits at 4 096
    a region); with them it is the budget. Both rows' worst cases are numbers G3′ measures (`g3h`:
    many seeds, several digs and a region of exactly the bound in one tick), not only means.
  - **The fall, in order, iterated until nothing moves.** Detached regions fall in ascending order
    of their lowest voxel's z, then of their identifying index; each region's drop is the smallest
    air gap under any of its voxels, **recomputed against the drops already made this tick**. One
    pass is not enough: two regions can interlock (one's lowest voxel lies below the other's while
    the other lies under part of it), so the first to fall can come to rest on a region that then
    falls away and leaves it floating. So the pass repeats, in the same order recomputed from the
    regions' current lowest voxels, until a pass moves no region. It ends: a region that moves comes
    to rest on something and, at that moment, nothing rests on it, so no two regions can ever hold
    each other up; and each pass that moves anything lowers some region by at least one voxel, so
    the passes are bounded by the tick's region count times the map's height (64), each pass's work
    by the regions' voxels, which the tick's visit budget already bounds. A pass count past that
    bound is a typed internal error, never a silent stop. A falling region's vacated voxels seed
    nothing, because anything face-connected to a detached region is in it. The region drops
    rigidly, every voxel leaving and returning through `set`, which marks its chunk (AGENTS.md
    §4.8). G3′'s adversarial tick (`g3h`) records the passes.
  - **What falls with it** ("everything", item 144 (1)). Every unit, structure, beacon, wreck, ruin
    and commander whose standing voxel or anchor column is in the region moves down with it; each of
    them that has hit points (units, structures, beacons, ruins, the commander) takes
    `collapse.fall_damage_hp` once a tick, however many passes move it, and a ruin destroyed that
    way is gone, while a wreck has no hit points, moves and keeps its salvage value. An asset under
    the voxels a region lands on is handled per decision 11. A commander's fall damage credits
    nobody, because a commander is not an asset (spec §4).
  - **Credit.** The fall damage is filed with the seats whose edits this tick removed the region's
    seeds' neighbours (`VoxelEdit`'s seat, which every edit a seat's asset files carries: a crater,
    a Mine dig, a terraform edit, a charge's cut): one seat takes it all; several seats share it per
    decision 37; the asset's own seat's share and a seat-less edit's credit nobody (item 144 (2);
    spec §3). It is applied at the end of the Voxels phase, and the settle function then books it,
    makes the wrecks and credits destroyed value inside Voxels, so the Match phase sees the death on
    the same tick (E2): this is the phase-order change the PR names.
  - **Place in the tick.** Inside `phase_voxels` (phase 10 of 12, after Pathing): apply the queued
    edits, re-checking each dig as it is applied (below); run the support search over this tick's
    removals; drop each detached region in order; apply and settle the fall damage; then settle the
    chunk digests once and refresh the pathing surface and mark the clusters of every settled chunk,
    as today (`world.rs` ~3109–3143). The falls therefore reach the hash in the same tick and the
    pathing graph through the existing path.
  - **The HPA\* cluster cache.** Collapse adds no invalidation path of its own: every voxel it moves
    goes through `set`, its chunk is settled, and `mark_chunk_clusters` marks that chunk's clusters
    for the next tick's Pathing repair (`pathing/repair.rs`), with the one-tick lag every edit has.
    A region spanning chunks marks each. A unit whose route crosses a changed column repaths through
    the router's existing check, under the cap of 16.
  - **Allocations.** None in the tick: a one-bit-per-voxel visited set and a label set over the
    whole map (about 1.2 MB each at 384 × 384 × 64, allocated once at construction and cleared by
    walking the list of bits each tick set), a FIFO queue and a region list each sized from
    `max_region_voxels` at load, a per-column height cache sized to the map's columns, and the seed
    list sized from the edit queue's worst case (E5). `crates/sim/tests/allocations.rs` gains a
    collapse every tick.
  - **Mine never undermines** (item 144 (3)), checked twice. When it chooses, the mining program
    runs the same bounded search read-only with the voxel treated as air; if any region it would
    detach carries a seat's structure or beacon, the voxel is refused and the drone picks the next.
    That check reads the last Voxels phase's state, so two digs, or a dig and a crater, on one tick
    could still detach such a region jointly; so the Voxels phase **re-checks each dig when it
    applies it**, against the state the edits before it in the queue have made, and refuses a dig
    that would detach a structure-carrying region, counted under its reason
    (`DigRefused::WouldUndermine`) and told to the drone, which picks again next tick. A dig may
    still detach a region that carries no structure, such as one an enemy unit stands on, and is
    credited like any other edit. Where a legal dig and a crater on one tick both seed the region
    that falls, the credit follows decision 37: split equally among the distinct seats whose edits
    that tick removed a voxel next to the region, the mining seat among them, the asset's own seat's
    share and a seat-less edit's crediting nobody. Pillar spacing counts as support for free: an
    undug column is solid to bedrock. Mine's searches count against neither row's budget but are
    bounded by `max_region_voxels` each, and G3′ measures them.
- **Builds:** the design above; events `region_collapsed` and `asset_fell` (E17); and, in place of
  a committed scenario, the staged integration test `collapse_credits_the_blast`
  (`crates/sim/tests/support.rs`), which `demo2`'s run sheet runs beside the demo: through test
  seams it builds a terrain shelf that stands on one face-connected neck of ground and carries a
  structure, a unit and the commander of seat 1, and has seat 0's mortar shells land so that two
  craters cut the neck; the shelf falls, everything on it takes the flat damage once, the kill
  credit goes to seat 0, and the test asserts the events, the state hash at named ticks and no
  dropped event (decision 39 says why the geometry has to be staged).
- **Implements:** items 138 (3), 144 (1) to (3). **Needs:** `fire` merged (the settle function, wall
  materials, the bore); the ruled decisions 11, 37 and 39. **Acceptance:**
  `a_region_cut_off_by_a_crater_falls`; `ground_beside_a_crater_is_supported_in_one_step`;
  `a_region_above_another_lands_on_it_and_does_not_float`; `a_region_larger_than_the_bound_holds`;
  `the_tick_budget_labels_the_rest_supported_and_counts_them`;
  `no_voxel_is_searched_twice_in_a_tick`; `the_bore_never_falls`;
  `everything_on_a_falling_region_falls_and_takes_the_flat_damage_once` (a unit, a structure, a
  beacon, a wreck, a ruin and the commander on it);
  `the_crater_seat_gets_the_kill_credit_and_an_own_crater_credits_nobody`;
  `two_seats_that_cut_one_region_share_its_credit`;
  `a_dig_that_drops_an_enemy_unit_credits_the_mining_seat`;
  `a_dig_and_a_crater_that_cut_one_region_split_its_credit`;
  `interlocking_regions_come_to_rest_and_none_floats`; `collapse_credits_the_blast`;
  `the_last_beacon_lost_to_a_collapse_ends_the_match_that_tick`;
  `a_collapse_death_on_a_segments_last_tick_reaches_its_recap`;
  `mine_never_digs_a_voxel_that_would_drop_a_structure`;
  `two_digs_that_would_drop_a_structure_together_are_refused_at_apply`; `pillars_hold_a_roof`;
  `collapse_allocates_nothing_in_the_tick`; chains re-blessed only if a committed run detaches a
  region (none is expected; one that does is named); no snapshot change (the rule holds no state
  between ticks).
- **Contract PR:** yes (determinism code: the Voxels phase and its settle; the event vocabulary).
  **PLACEHOLDERs:** none new (the constants are `con3`'s rows). **Agent-days:** 6.25–7.25 (a
  quarter-day of it for the iterated fall, the dig's credit and the scratch field, from the
  verification review).

### `view` — `crates/client-gdext` + `godot/` + `crates/mesher`: drawing combat

- **Owns:** `crates/client-gdext`, `godot/`, `crates/mesher`, their goldens. **Branch**
  `feat/client-combat`, worktree `../pharmakos-view`, target `D:/build/view`.
- **Builds:** HP on own assets and on visible enemies as decision 5 rules, from the view's fields;
  shells, wrecks and ruins as entities; a unit's heading where the view carries it; the wall
  materials of `con4`'s table drawn by the seat each material names, whole or cracked, which is
  who built the wall (whose it is now, neutral or not, is the sim's reading, which the client may
  not make: E14, decision 42), and bedrock drawn as rock; the cracked terrain materials
  drawn as cracked versions of their base (decision 10), so that a cracked stone voxel, which today's
  `palette_of` would draw as air, is ground, through new rows in the mesher's `PALETTE` (art
  placeholders for S6) and `palette_of`'s mapping of every new id, pinned by a test that walks the
  wire table; the Attack beacon's target
  drawn in the editor (`editor.rs` ~176); `ResolvedRef.reachable` drawn (`targeting.rs` ~246); the
  remesh under a crater stream: today more than 16 changed chunks re-bakes the whole map (`view.rs`
  `WHOLE_BAKE_ABOVE`, ~297, ~635) and one edit's light pad of 15 dirties more chunks than G1
  measured (`mesher/src/bake.rs` ~35–45), so the lane measures a 20 and a 40 explosions-a-second
  stream through the mesher's headless CPU proxy (G1's own stress step, which "S2 will need") on all
  three runners and replaces the whole re-bake with a bounded incremental relight if the proxy shows
  it misses G1's three frames; the stream's case added to `crates/mesher/tests/perf_alarm.rs` (the
  `perf_alarms` entry that publishes it is `g3h`'s, which holds `xtask` next); item 136 (5)'s
  `NOT_FOUND` narrowing decided and `editor.rs`'s doc brought to it (section 2.2 says why here and
  not in `rnds`); `client-gdext/src/rules.rs`'s stale comment (~283); E7's sweep over
  `crates/client-gdext` (34 counted) and `crates/mesher` (33 counted); E28's per-frame cost of the
  full entity list. Every new piece that schedules or draws without deciding is named in the PR for
  AGENTS.md §3 rule 4. Nothing here decides, times or validates (rule 4).
- **Also (decisions-log item 146 (4), from `rnds` #124):** `host_link.gd`'s `--gamectl=` and
  `--root=` readers silently take the first match of a repeated option, where `rnds`' `--rounds`
  reader refuses a repeat (`RoundsRefusal.REPEATED`); this lane gives them the same refusal, with
  its words and its `godot_project.rs` pins.
- **Implements:** register S2-12's measurement; G1's stress headroom; item 144 (11)'s cracked
  terrain, drawn. **Needs:** `wgw` and `fire` merged (the client draws an unknown material as air,
  so it waits for both the wire's table and the sim's materials, E14); the ruled decisions 5, 10
  and 42.
  **Acceptance:** vista and rows PNGs re-rendered on CI's Linux leg and looked at;
  `godot_project.rs` pins; `every_wire_material_draws` (no id the wire table names maps to air but
  air itself); the mesher's perf alarm case for the stream; `no_silent_fallback_is_left` in both
  crates; the watch check by hand; the main session's local package run. **Contract PR:** no.
  **Agent-days:** 6.25–7.75 (a quarter-day of it for the cracked terrain's drawing).

### `g3h` — `crates/bench` + `xtask`: the G3′-real harness

- **Owns:** `crates/bench` and `xtask/src/main.rs`'s `perf_alarms` entries (its own figures and
  `view`'s mesher stream). It holds no sim or gateway place: the seams it needs are `base`'s (E30).
  **Branch** `feat/bench-g3-real`, worktree `../pharmakos-g3h`, target `D:/build/g3h`.
- **Builds:** section 5's harness, driving E30's load generator: the bench rules variant and the
  gate's layout (decision 2's seats, 300 units and 40 beacons as world totals, item 63), with real
  combat, real pathing (no charged substitute; item 64: G2's CSR rebuild counted once) and a crater
  stream at the destruction rate the budget is set with; per-phase tick timings through the phase
  stepper; the tick's p50, p99 and mean, ticks a second, the tick-minus-pathing mean (item 116 (6)
  (b)), the repath backlog against the cap, the broadphase's share with E11's pad, the support
  search's adversarial tick (many seeds from several charges, several digs whose Mine checks run in
  Programs, and a region of exactly `max_region_voxels`) with the fall's passes on that tick
  (interlocking regions included), the per-tick visit budget's use,
  kill-credit settlement at the run's death rate, the peak events per tick against the bus's 512
  (E15), the damage queue's bytes (E5), and live heap bytes over a full 8-minute segment through a
  counting global allocator (item 68; std-only, so no dependency) with tick p99 per minute (spike
  step 6's drift check); every figure published by `cargo xtask perf-alarms` as notices;
  `tests/harness.rs`' doc list of restated items extended (item 133 (5)); E7's sweep over
  `crates/bench` (4 counted).
- **Implements:** items 63, 64, 68, 69, 116 (6) (b), 138 (3) (the worst case). **Needs:** `base` and
  `fire` merged to start (it merges `main` in after `site` and `fall` before it opens its PR, and `g3r`
  re-points it at the finished sim); `view` merged (its `perf_alarms` entry); the ruled decision 2.
  **Acceptance:** `wall-guard` and the research guard green; the layout and the rules variant
  reproducible from the report's seed and `rules_hash`; the notices on all three runners.
  **Contract PR:** yes (`xtask`). **Agent-days:** 4.5–5.5.

### `dfnd` — `crates/sim`: Defend, the threat model, the bodyguard

- **Owns:** `crates/sim`, every golden it moves, the preamble's named places (the Defend and
  `beacon_under_attack` rows of `mandate_and_rule_gaps()`), its scenarios' entries. **Branch**
  `feat/sim-defend`, worktree `../pharmakos-dfnd`, target `D:/build/dfnd`.
- **Builds:** the threat model per decision 12 (`most_threatened`, `beacon_under_attack`, the Defend
  urgency band's trigger, `safest`'s threat term (S2-04), `SAFEST` as a seam choice,
  `flee_on_threat` for Mine); `safest`'s repair term, which makes the reflex and the safe playbook's
  shadowing a rescue (decision 6); the Defend mandate: threat-weighted targeting inside the
  engagement radius, pursue as set, the protect list with `my_commander` (guards escort the
  commander and intercept threats near it while it is inside the sphere), damaged units falling back
  to the nearest friendly repair drone, the picket (scouts patrolling `defend.picket_radius_voxels`
  outside the sphere edge, through the sensor seam); raiders fielded as guards on work in hand (a
  threat seen in the sphere); decision 13's deferrals refused with their E-codes. Defend's state
  (guard assignments, picket legs) is a hashed table of its own with a row per Defend beacon, and
  **this lane also lays out `surv`'s per-seat explored-cell table** (its type, its block in the
  hash, its snapshot field and restore), written by nothing yet, so that `surv` touches no hash,
  snapshot or table line; both tables hash nothing while empty (E29), so a match with no Defend
  beacon and no explored cell hashes as before, and one snapshot bump covers both. Scenario
  `scenarios/s2/guards-escort-the-commander`.
- **Also (decisions-log item 145 (5)):** `initial.mandate {defend: {}}` is refused
  (`compile_settings_row`) while an empty Defend arm in a `set_mandate` is legal
  (`compile_switch_settings`), and spec §5 makes the mandate type a free choice at deploy. This lane,
  which makes Defend's settings real, accepts an empty Defend arm in `initial` too, and the
  verifier follows the sim (the main session's call, open to the owner's veto). The verifier mirrors
  the sim there and no verifier lane runs in wave 4, so this lane is granted
  `crates/verifier/src/refusals.rs`' E0012 entry, with its case and goldens, as named places.
- **Also (decisions-log item 146 (4), from `vfy2` #125):**
  - a protect-list selector ranking `ENEMY_KNOWN` raises E0415 in the verifier and would raise
    E0015 at the sim once the arm compiles: the lane keeps one of the two and names which in its
    PR, the verifier's E0415 check for the protect list, with its case and goldens, being a named
    place if the lane drops it;
  - pickets are spent as ordered scouts in the verifier's projection, as Survey's are, and the sim
    spends them the same way;
  - when E0012 lifts, `/defend/` leaves `refusals.rs`' `not_compiled` skip list, through the E0012
    places above;
  - the picket ceiling borrows Survey's `u8` scout column until Defend's own column exists
    (`crates/verifier/src/semantics.rs`' marker, register S2-18): the lane gives Defend its column
    and discharges the marker, or re-dates it.
- **Implements:** items 104, 138 (4) (c) (pickets); spec §6's Defend row, §4's bodyguard, §10's
  reflex; register S2-02 (the sim's half), S2-04. **Needs:** `site` and `fall` merged; the ruled
  decisions 6, 12, 13 and 21. **Acceptance:**
  `the_most_threatened_beacon_holds_the_most_threat`; `under_attack_reads_its_window`;
  `guards_escort_the_commander_inside_the_sphere_only`; `a_damaged_guard_walks_to_a_repair_drone`;
  `pursue_never_stops_at_the_engagement_radius`; `a_picket_sees_past_the_sphere`;
  `safest_seam_avoids_a_known_threat`; `a_wounded_commander_is_walked_to_a_repair_drone`;
  `a_mine_drone_flees_a_threat`; `an_empty_new_table_hashes_nothing`;
  `the_explored_table_survives_save_and_restore`; the gateway's two door rows flipped; W0501's
  warning lifted in the verifier through the named place; the scenario; chains re-blessed if any
  committed run reads `safest` with a threat now in view or a wounded commander (`against-easy*`
  may); the next `SNAPSHOT_VERSION`, every `report_hash` golden with it.
- **Contract PR:** yes (determinism code: two new tables' hashing, the snapshot). **Agent-days:**
  6.5–7.5.

### `surv` — named place in `crates/sim`: full Survey

- **Owns:** `crates/sim/src/survey.rs`, the Survey half of `crates/sim/src/sight.rs`, a new
  `crates/sim/tests/survey.rs`, its kinds at the end of `EventKind` with their gateway arms (E17),
  and its scenario's entries. It writes the explored-cell table `dfnd` laid out, through that
  table's API, and edits no line of `hash.rs`, `snapshot.rs` or `tables.rs`. It runs beside `atk`,
  which lists these as excepted. **Branch** `feat/sim-full-survey`, worktree `../pharmakos-surv`,
  target `D:/build/surv`.
- **Builds:** spec §6's Survey row: scouts scan unexplored ground inside the probe areas first (the
  per-seat explored table over coarse cells); then re-check the sightings closest to leaving the
  reach window, enemy beacons first, refreshing them before the next Lull (item 13; this refresh
  pass is never cut, on the owner's word, item 144 (11), so it ships in S2 whatever happens);
  avoiding known threats (decision 12); reporting back through the feed. Scenario
  `scenarios/s2/survey-finds-the-enemy`.
- **Implements:** spec §6's Survey row; item 13's refresh; item 138 (4) (c). **Needs:** `dfnd`
  merged; the ruled decision 12. **Acceptance:** `scouts_scan_unexplored_ground_first`;
  `the_stalest_sighting_is_refreshed_next`; `enemy_beacons_come_first`; the scenario; chains
  re-blessed where a committed run has a Survey beacon whose scouts now mark cells (the table goes
  non-empty), each named; no snapshot change. **Contract PR:** no (behaviour writing into a table
  whose hashing `dfnd`'s contract PR set). **Agent-days:** 3–4.

### `vfy3` — `crates/verifier`: the threat model's lints

- **Owns:** `crates/verifier`, its goldens. **Branch** `feat/verifier-threats`, worktree
  `../pharmakos-vfy3`. **Builds:** W0501 retired with its catalogue row (SAFEST means something
  now), if `dfnd` has not done it in its named place; an I-level note where `AVOID_KNOWN_THREATS`
  has no sighting in `Scope` to avoid and so routes as `DIRECT` (a new code, the editor's
  explanation and no more); the prose and catalogue text for SAFEST and `flee_on_threat` brought to
  decision 12; E0209 stays S3's (the enemy-knowledge predicates); and, from `vfy2` #125
  (decisions-log item 146 (4)), a half-written general `Area` (a protected, probe or avoid area
  missing a corner) refused as E0101 outside a wall's box, as a wall's box already is, with a case.
  **Granted** (decisions-log item 145 (5)): `crates/gateway/tests/{methods,advice,economy}.rs` and
  `tests/golden/docs/reference/expected.docs.txt`, which a new catalogue row moves. **Needs:** `dfnd`
  and `vfy2` merged; decision 12. **Acceptance:** a case per code; the catalogue golden. **Contract PR:** no.
  **Agent-days:** 1.5–2.

### `atk` — `crates/sim`: Attack, hunting, breach and the demolition charge

- **Owns:** `crates/sim` except `surv`'s named places, every golden it moves, the preamble's named
  places (the Attack row of `mandate_and_rule_gaps()`), `crates/sim/tests/allocations.rs`, its
  scenarios' entries. **Branch** `feat/sim-attack`, worktree `../pharmakos-atk`, target
  `D:/build/atk`.
- **Builds:**
  - **The Attack mandate** (spec §6): raiders fielded on work in hand; gather at the staging point;
    launch when the force size or the segment time holds (the go-code arm stays refused as S4's);
    attack the weak point (decision 14); retreat on its conditions; the after-action; avoid areas as
    a route cost; waves (the first cut, section 4.5). The target is pinned at the seal and launch
    never re-tests it (item 13; `interpreter.rs` ~245; `targeting.md` ~68); "if the remembered
    target has moved or died, the after-action or retarget rule decides on arrival".
  - **Commander hunting:** an Attack target of `enemy_commander` starts from the last sighting and
    retargets live while any of the seat's units sees it, reading the same reach as plan time.
  - **`AVOID_KNOWN_THREATS`** (S2-03): a route cost surcharge per step inside a known threat's
    weapon range, from the threat model and the rules row.
  - **Breach** (E23, decisions 9, 40 and 41): with `breach_allowed`, an Attack whose route is sealed
    by walls queries the pathfinder's breach mode for the cheapest way through, asks the seat's
    mortars in range of the first wall column on that route to shell it (enemy wall voxels are
    targets only for a breach, decision 41), and sends a raider carrying a demolition charge
    (decision 9) to that column, resuming once the gap opens; the charge is bought at
    `demolition_charge_cost_dollars` when it is loaded, planted on arrival, and detonates when its
    fuse row runs out, cutting wall and terrain inside its radius flat down to the wall's footing
    (decision 40), whatever each voxel's level, cracked or whole, stone included (decision 10:
    a cut that left stone standing would leave a step), with the carrier's seat on the edit, so the
    gap is walkable; the cut stops above bedrock and the bore (E14). Because a wall is filled down
    to the ground (`site`), the flat cut leaves nothing above it unsupported, so a breach never
    causes a collapse (decision 39). Intercepting the carrier is the counter: a carrier killed
    before it plants drops a charge that never fires (decision 9).
  - **Attack's state** (the pinned target, the staging and launch state, the wave count, a charge's
    carrier and fuse) in a hashed table of its own with a row per Attack beacon, hashing nothing
    while empty (E29).
  - **A moving crowd's allocations** (item 137 (2) (j)): `allocations.rs` gains an attacking crowd
    under combat, and its fixed-stride check reads the live-row tables E1 made dense.
  - **Scenarios:** `scenarios/s2/hunt-the-commander` and `scenarios/s2/breach-and-eliminate` (a
    breach whose route reopens through the charge's cut, asserted by a raider walking the gap; a
    beacon destroyed into ruins; the last beacon lost; the match ending `last_seat_standing` on that
    tick); and the two that `fire` could not commit, because this is the first lane whose playbooks
    reach them (`fire` keeps their staged twins as integration tests):
    `scenarios/s2/raid-on-a-mortar` (an Attack beacon's raiders dive a mortar the other seat's Build
    beacon built, the autocannon and the core kill them, wrecks left) and
    `scenarios/s2/mortar-breaks-wall` (an Attack beacon with `breach_allowed` whose route a wall
    seals asks its seat's mortar to shell the wall's first column, which cracks on the first shell
    and breaks on the next, craters asserted, before the charge arrives). Each asserts events,
    hashes and no dropped event, and this lane owns their path constants, `SCENARIOS` entries and
    lines in `the_committed_list_is_the_whole_set`.
- **For the owner, before the lane starts** (decisions-log item 145 (5)): which enemy commander an
  Attack on "the enemy commander" (`con4`'s `AttackTarget.enemy_commander`) picks when there are
  three seats. The lane puts the question with its options and stops for the answer.
- **Also (decisions-log item 146 (4), from `vfy2` #125):**
  - E0418's commander rule follows the owner's answer to that question; today any enemy
    commander's sighting inside `verifier.reach_memory_ms` qualifies;
  - the lane needs `wgw` merged (`Scope`'s sightings) before freshness means anything in a real
    match;
  - an empty Attack arm in a deploy's `initial` is refused today (E0013, as the sim refuses it):
    whether it becomes legal, as Defend's will, is this lane's to settle; lifting E0013 drops
    `/attack/` from `refusals.rs`' `not_compiled` and prices the fields in the sim's
    `mandate_fields` as `interface.rs` does.
- **Implements:** items 13, 104; spec §4 (hunting), §6's Attack row, §9 (breach, friendly fire);
  register S2-03. **Needs:** `dfnd` merged; the ruled decisions 9, 10, 12, 14, 40 and 41.
  **Acceptance:** `an_attack_waits_for_its_force_size`; `the_target_is_pinned_at_the_seal`;
  `a_hunted_commander_is_retargeted_while_seen_and_lost_when_not`;
  `avoid_known_threats_routes_around_range`; `a_breach_sends_a_charge_to_the_wall_and_resumes`;
  `a_charge_cuts_flat_to_the_footing_and_the_gap_is_walkable` (through whole stone);
  `only_a_breach_shells_an_enemy_wall`; `a_carrier_killed_before_it_plants_fires_nothing`;
  `a_charge_crater_carries_its_seat`; `a_breach_cut_drops_nothing`;
  `the_last_beacon_lost_ends_the_match_that_tick`; the gateway's door row flipped; `allocations.rs`
  green with the crowd; the four scenarios; chains re-blessed where Easy's playbooks now attack
  (none of the committed ones is expected to; any that moves is named); the next
  `SNAPSHOT_VERSION` for Attack's state.
- **Contract PR:** yes. **Agent-days:** 9.5–11 (half a day of it for the two scenarios moved from
  `fire`, from the verification review). **Split seam:** the mandate, the pin and hunting as set 1;
  breach, the charge, avoid areas and waves as set 2; the four scenarios last.

### `oprd` → `opra` — `crates/operator` + `library/`: Easy under combat, one lane in two PRs

- **Owns:** `crates/operator`, `library/`, `tests/golden/operator`, and the named places AGENTS.md
  §6 grants a lane that changes the operator's output: `crates/gamectl/tests/{operator.rs,
  operator_host.rs, wizard.rs}`, `crates/gateway/tests/advice.rs`,
  `crates/client-gdext/tests/wizard.rs`, the gateway's `instantiate_suggested` golden and
  `godot/fixtures/instantiate_suggested.json`; and, because a new template ships and Easy's sealed
  files change, the zips' manifests `tests/golden/package/expected.{windows,linux}.txt` (a contract
  path: the new `library/*.jsonc` entries), `crates/gateway/tests/templates.rs`' `SHIPPED` list
  (~27), the new templates' `demo_*_verify` and `demo_*_render` goldens beside the existing ones in
  `tests/golden/gateway/`, `demo_against_easy_render` and `demo_against_easy_verify`, and the
  `against-easy` and `against-easy-three-rounds` chains under `tests/golden/scenarios/`, and the
  nightly result golden `tests/golden/scenarios/nightly-result/`, which Easy's output moves
  (decisions-log item 145 (5)). No gateway
  or gamectl lane is running at either PR's start. **Branches** `feat/operator-defend` then
  `feat/operator-attack`.
- **`oprd` builds** (after `dfnd`): the Defend template in `library/` (guard the core and "my
  commander"); Easy's defence score from the threat model; a Defend guard on its commander in the
  round after a Push in which its commander died (spec §14; decision 35, which moved it from S5 to
  S2), with `lib.rs`' marker (~61) re-dated for the three parts that stay S5's (the Survey post,
  chatter and target spread); S2-02's rescue per decision 6, which needs no new
  vocabulary in the safe playbook or Easy's output (it rides `safest`); Hold & Build's
  `repair_threshold_pct` per decision 38; `wire.rs`' defaults (`int_of`, `text_of`, `bool_of`,
  `array_of`, `voxel`) and `candidates::score`'s `time_ms.max(1)` made typed reads (item 136 (5)),
  which is E7's sweep for the crate; Expand & Mine's "ore seam" → "scrap seam" (spec §9). 3–3.5 ad.
- **`opra` builds** (after `atk`): the Attack template (raid the nearest known enemy beacon); Easy's
  attack score (`candidates.rs` ~54–55, "aiming at an enemy is S2's") and enemy-commander raids
  scored like any other target (spec §14); the wizard's suggestions under both templates. 2–2.5 ad.
- **Implements:** spec §14 (S2's share); register S2-02. **Needs:** `oprd`: `dfnd` merged and the
  ruled decisions 6, 12, 35 and 38; `opra`: `atk` merged and the ruled decision 14.
  **Acceptance:** the templates
  round-trip, verify and render identically; the operator goldens, the wizard's,
  `instantiate_suggested`, the `demo_*` goldens and the package manifests moved and explained;
  `templates.rs` green over the longer `SHIPPED`; `against-easy*` re-blessed when Easy's sealed
  files change; the main session's local package run (`library/` ships). **Contract PR:** yes, each
  (`tests/golden/package/`, the zips' manifests). **Agent-days:** 5–6 in all.

### `arms` — the three nightly arms and their bands

- **Owns:** `scenarios/nightly/` (after `night`). **Branch** `feat/scenarios-nightly-arms`, worktree
  `../pharmakos-arms`.
- **Builds:** Rusher (an early Attack beacon, launch on a small force size against the nearest known
  enemy beacon, then the core), Turtle (Defend beacons on the core and "my commander", walls and
  autocannons, repair drones, hoarding), Hunter (Survey for the commander, an Attack targeting
  `enemy_commander`), each a hand-written two-seat playbook in v1 vocabulary (spec §15; decision
  36), checked by `night`'s per-PR test; `seeds.txt` with twenty seeds, the balance check's twenty
  (S1-53, item 139 (1) ruling 5), so the two reports share maps, committed and never regenerated;
  the written bands in `scenarios/nightly/README.md` per decisions 18 and 29; a forced nightly run
  on its branch, its report attached; `demo2` forces it again on `tune2`'s numbers.
- **Implements:** spec §15; item 144 (6); AGENTS.md §10 item 5. **Needs:** `night`, `atk` and `opra`
  merged; the ruled decisions 18, 29 and 36. **Acceptance:** each arm qualifies; the
  forced run's three results; every band written. **Contract PR:** no. **Agent-days:** 2–3.

### `g3r` — the certifying runs, the alarm job and the re-derived power rows

- **Owns:** `crates/bench` (after `g3h`), `xtask/src/main.rs` (`g3-budget`),
  `.github/workflows/ci.yml` (the new job and the perf-alarms comment, ~986). **Branch**
  `feat/g3-real-gate`, worktree `../pharmakos-g3r`.
- **Builds:** the harness re-pointed at the finished sim (`atk` and `surv` merged); **the certifying
  runs on the owner's machine (i7-9800X), under Windows and under WSL2 Linux** through
  `C:/Users/PC/ClaudeProject/bin/wsl-ci.sh`, three repetitions each with the median of the p99s and
  the clocks recorded before and after (spike G3′'s thermal rule), the slower OS deciding, recorded
  in the PR with the three runners' notices beside them; E18's command and alarm job; the re-derived
  inputs for `tune2`, every one stated: the per-map power budget and the three power rows that move
  together (items 65, 91), the destruction rate set with them, the repath cap from the measured
  burst frequency (item 69's window 15–16, floor 9.45), the broadphase cell size, the fielding
  totals (S1-37, S2-08), and `max_region_voxels` and `max_visits_per_tick` if their worst cases do
  not fit; G4's bar's first nightly result cited. If G3′ misses, the written fallback (section 5).
- **Implements:** items 63–69, 91, 116 (6) (b), 138 (3); spec §16's G3′ row; AGENTS.md §9 item 11.
  **Needs:** `atk`, `surv` and `g3h` merged; the ruled decision 2. **Acceptance:** the
  job green on its branch; the recorded figures from both OSes; the proposed rows handed to `tune2`
  in the PR body. **Contract PR:** yes (`xtask`, a workflow). **Agent-days:** 2.5–3.5.

### `tune2` — the numbers, the power budget and the walkers

- **Owns:** `rules/rules.v1.json` values and `rules/README.md`; `crates/gamectl/tests/balance.rs`
  and a new `#[ignore]`d `crates/gamectl/tests/duels.rs`; S2-16's walker removal as named places —
  `crates/gamectl/src/scenario.rs` (`units_per_seat`, `MAX_UNITS_PER_SEAT`),
  `crates/gamectl/src/scenario/run.rs` (~47, ~391), `crates/gamectl/src/doctor.rs` (~326, ~462),
  `xtask/src/scenario.rs` (its twin), `crates/sim/src/world.rs` (`WorldConfig::units_per_seat` and
  the walkers; no sim lane is running), and `scenarios/README.md`'s key; and, if `g3r` moves the
  fielding totals, which are world constants and not rules rows, those constants as named places in
  `crates/sim/src/world.rs` (`UNIT_TABLE_ROOM`, `BEACON_TABLE_ROOM`, `STRUCTURE_TABLE_ROOM`,
  ~153–192, with their markers and the ruin reserve E1 sizes from the last); the rules-hash pin
  `crates/sim/tests/determinism.rs:811` and the nightly result golden
  `tests/golden/scenarios/nightly-result/`, which every rules-table change moves (decisions-log item
  145 (4) and (5)); every chain and golden its values move. **Branch** `feat/rules-s2-numbers`, worktree `../pharmakos-tune2`.
- **Builds:** the balance check extended to combat (Easy against Easy and each template against
  Easy, 20 seeds, 3 and 6 rounds; per seat per round kills, losses, value destroyed, the time to the
  first kill, and how each match ended), with a **three-seat** run (three Easy seats, 20 seeds) that
  reports, per seed and in sum, the share of all damage aimed at the seat trailing in the Ledger's
  standing when it was dealt, from `fire`'s seat-against-seat counter (decision 36; spec §19's
  kingmaking check, for S2); the spawn travel distance in raider and commander time between the
  generator's spawn pairs, against spec §19's "about 1.5 early Pushes"; reach memory N against
  Survey's refresh interval; a duel report holding the kit's counter relations (spec §9's table:
  autocannon against raiders, mortar out-ranging the autocannon, raiders diving a mortar, a mortar
  against a clump and a wall, and the shells a crater in dirt and in stone takes at the proposed
  `blast_levels`, decision 10), each relation a line the owner reads, never a gate; every combat row
  proposed per decision 17 with its evidence, the picket radius and the blast levels included;
  `g3r`'s power rows, repath cap, cell size and totals; S2-16: `units_per_seat` and its cap removed
  from the scenario format (a format change, every committed file has it absent), the walkers
  deleted from the sim, since G3′'s load comes from E30's layout and never used them (decision 23);
  the rules text's note updated.
- **Implements:** item 144 (8); register S1-37, S2-08, S2-16, the S2 tuning rows; spec §19's S2
  items. **Needs:** `opra` and `g3r` merged; the ruled decisions 17, 23 and 36.
  **Acceptance:** both reports attached, the three-seat figure in the balance report; every chain
  and `report_hash` golden re-blessed once, the rows named; `mapgen`'s goldens re-blessed if
  `map_ceiling_kw` moves the generator; the main session's local package run. **Contract PR:** yes
  (the scenario format, `xtask`; `rules.proto` only if a row retires). **Agent-days:** 4.75–5.75.

### `demo2` — the run sheet, the certification and the register

- **Owns:** `docs/demo/s2-run-sheet.md` and the register's S2 section (the main session's docs PR).
  **Builds:** the run sheet for §1.1, with `FREE_FIRE` set on the owner's Build beacon in round 1's
  Lull (step 1), and decision 39's collapse beat shown staged: `fall`'s `collapse_credits_the_blast`
  run headless beside the zip and its event log shown, the sheet saying plainly that the beat is
  staged and why; G3′'s
  certification **re-run on `tune2`'s final numbers** on the owner's machine under Windows and WSL2
  Linux and recorded, since `tune2` moves the power budget and the rows the gate load reads; the
  nightly report with each band acknowledged as Easy's; the register's S2 rows closed or moved with
  their owners. **Implements:** AGENTS.md §10 items 5, 6 and 8. **Needs:** `tune2` and `arms`
  merged. **Agent-days:** 2–3.

---

## 4. The schedule

### 4.1 Waves

Three slots, never more. `crates/sim` has one author at a time; beside it run only the named places
`fall` (beside `site`) and `surv` (beside `atk`), each a new or separate module with call lines and
no hash, snapshot or table line of its own (`fall`'s one unhashed scratch field on `World`
included), as `grid` ran beside `mine` in S1, and `con4`'s sim place beside `base` and then `ids`
(the Defend and Attack arms of `compile_settings_row` in `interpreter.rs` and their refusal-table
entries, a few lines in an existing file, the owner listing them as excepted). Days are working days
in plan units, five to a week, at each task's midpoint estimate; a task starts the day its inputs
merge, and a lane opens its PR when green, so the order inside a wave is a merge order. **The bulk
ruling** fell on 2026-10-16, before wave 1 (item 144 (11)), so no lane waits on it; the plan had it
fall after `msrv` merges (day 0.75) and before `con3` starts (day 2).

| Wave | Days | Slot 1 — sim | Slot 2 | Slot 3 | The owner can see |
|---|---|---|---|---|---|
| 0 | 0–2 | **msrv** (0.75) → **base** starts | → **redate** (0.75) | → **east** (1.25) | 1.98 everywhere; the `scenarios/` comments (`expand-east-segment`'s nearer site lands with `base`, item 145 (2)) |
| 1 | 0.75–14 | **base** (7) → **ids** (6.25, from 7.75) | **con3** (4.25, from 2) → **con4** (3.75, from 6.25) → **vfy2** starts (10) | **vfy1** (3.75, from 2) → **night** (5.25, from 5.75) → **rnds** (0.75, from 11) | `--rounds 6`; E8's verdicts agree with the compile; live fielding; the nightly harness forced once |
| 2 | 14–26 | **fire** (12) | **vfy2** (6, to 16) → **wgw** (6.5, from 16) | idle by dependency from 11.75 | Shells, craters, cracked stone, wrecks, kill credit |
| 3 | 26–34.5 | **site** (8.5) | **view** (7, from 26) → **g3h** (5, from 33) | **fall** (6.75, from 26, named place) | A wall going up, levelling, repair and salvage, the support rule (staged); combat drawn |
| 4 | 34.5–41.5 | **dfnd** (7) | **g3h** (to 38) | idle by dependency | Guards, pickets, the threat model, the rescue |
| 5 | 41.5–51.75 | **atk** (10.25) | **surv** (3.5, named place) → **vfy3** (1.75, from 45) | **oprd** (3.25, from 41.5) | A raid, a hunt, a breach, a charge; full Survey |
| 6 | 51.75–62.5 | **g3r** (3) → **tune2** (5.25, from 54.75) → **demo2** (2.5, from 60) | **opra** (2.25) → **arms** (2.5, from 54) | — | The S2 demo of §1.1 |

`rnds` runs after `night` rather than before it because `con3` and `con4` hold named places in
`crates/client-gdext` until day 10, `con3`'s in `rnds`' own `godot_project.rs` (section 3's
preamble); slot 3 ends on day 11.75 either way.

The idle stretches are by dependency, not by choice: during `fire` nothing off the sim's path is
ready once `rnds` and `wgw` are done, and during `dfnd` only `g3h` is (it starts after `view`,
which it follows in `xtask`, and builds against the combat it measures rather than starting earlier
and being re-pointed twice). A lane that slips takes from that slack first.

**Totals.** 27 tasks in 26 lanes (`oprd` and `opra` are one lane in two PRs), **about 128 ad**
(115.5–142), including about 1.5 ad of re-blesses after merges of `main` (E33), about 1.75 ad for the
owner's per-material hardness (decision 10: `fire` 1.25, `con3` 0.25, `view` 0.25) and about 2 ad
for the verification review's fixes (`ids` 0.75, `atk` 0.5, `base`, `site`, `fall` and `vfy2` a
quarter each, `fire` a quarter less). **Critical path:** `msrv` → `base` → `ids` → `fire` →
`site` → `dfnd` → `atk` → `g3r` → `tune2` → `demo2`, **about 62.5 working days**, almost
all of it the sim's serial lanes (`base` to `atk`, about 51 days); per-material hardness adds 1.25
days to it, through `fire`, and the verification review's fixes 1.5. `opra` runs beside `g3r` and is
shorter, so it is off the path. Slot utilisation is about 68 % of 187.5 slot-days.

**Token cost.** At the skeleton's wave-4 and wave-5 rates of 186–195 k sub-agent tokens per
planned agent-day (item 115 (2)), 128 ad is about **24–25 M** tokens; at wave 6's 215–245 k,
27.5–31.5 M. The record holds no S1 rate to set beside them.

### 4.2 The re-bless ledger: how many times the chains move, and why

| PR | Scenario chains | Determinism chain | `report_hash` goldens |
|---|---|---|---|
| `east` | — (cut to comments, item 145 (2)) | — | — |
| `vfy1` (`VERIFIER_VERSION` 0.1.0-s2) | — | — | all |
| `con3` (rows at today's constants; `blast_levels`, which nothing reads yet) | — | — | all (`rules_hash`), after `vfy1`'s |
| `base` | `expand-east-segment` and its event log, for its own playbook (item 145 (2)); no other (no hashed field, no snapshot change) | — | — |
| `ids` (the placement counter and beacons-lost columns) | all | **moved** | all (snapshot 10) |
| `vfy2` (`Scope`'s sightings) | — | — | all, after `ids`' |
| `wgw` (the scope's content) | — | — | the gateway's, after `vfy2`'s |
| `fire` (combat columns; the core's cooldown in every match; bedrock in the bottom layer, which moves the mapgen digest goldens too; the material levels move nothing of their own, decision 10) | all (its staged cases are `crates/sim/tests`, so it adds no committed scenario) | **moved** | all (snapshot 11) |
| `site` | those with a Build, if drone state is hashed; its two new ones | — expected (Hold & Build's terraform reads NONE and nothing in it is damaged) | all if the snapshot bumps |
| `fall` | none expected (no committed run detaches a region; its staged cases are `crates/sim/tests`) | — expected | — |
| `dfnd` (Defend's table; the explored table laid out) | those reading `safest` or `SAFEST` with a threat in view or a wounded commander; its new one | — expected: both new tables hash nothing while empty (E29) | all (snapshot) |
| `surv` (the explored table written) | those with a Survey beacon whose scouts now mark cells (`brownout-by-recycle`, `round-limit-audit` and `unaffordable-deploy` name one, each moving only if its Survey beacon is placed); its new one | — expected (the determinism playbook seals no Survey beacon) | — |
| `atk` | its four new ones (two moved from `fire`); any where Easy now attacks | — expected (E29) | all (snapshot) |
| `oprd` (Easy's sealed files; Hold & Build's threshold) | `against-easy*` | — | the operator's and the `demo_*` |
| `opra` (Easy's sealed files) | `against-easy*` | — | the operator's and the `demo_*` |
| `tune2` (values; walkers removed; the power budget) | all | **moved** | all (`rules_hash`) |

**Over the stage the scenario chains move three times for certain** (`ids`, `fire`, `tune2`) and up
to six more (`site`, `dfnd`, `surv`, `atk`, `oprd`, `opra`), **the determinism chain three times**
(`ids`, `fire`, `tune2`), and every `report_hash` golden eight times for certain (`vfy1`, `con3`,
`ids`, `vfy2`, `fire`, `dfnd`, `atk`, `tune2`: the fixture snapshot is one of its inputs, so each
snapshot bump moves them, AGENTS.md §4.8, and so does `VERIFIER_VERSION`) and a ninth if `site`'s
drone state bumps the snapshot. Where two running lanes move the same set, the one merging second
re-blesses after its merge of `main` (E33). Each move is one behaviour change reviewable alone, and the
numbers come last because the check needs the finished rules.

### 4.3 Honest size

About 128 ad and a 62.5-day critical path in plan units, against the spec's 6 weeks (30 days);
with a week of float for contract reviews, about **13.5 plan-weeks**. The sim alone carries about
61 ad, about 51 of it in series. S1's plan sized its row at about 7.1 plan-weeks against the
spec's 5, about 40 % over; S2's row is larger than S1's in every layer, and the §2.7 additions
(collapse, terraform, Defend's and Attack's full settings, the core's turret) are about 15 ad of it.
The two reviews added about 18 ad and 11 days of path: the id design E1 needs and the whole-sim
fallback sweep (which made `base` two lanes, `base` and `ids`), the combat and collapse corrections
(the phase in which a collapse's death settles, the stale broadphase, the fall's order, Mine's
re-check, the per-tick budget, the bore, the shared credit, the attacker memory), the harness's
seams and load generator, the sweep's other crates and the two-OS certification. The owner's
rulings (item 144 (11)) then added about 1.75 ad and 1.25 days of path for per-material hardness
(decision 10), almost all of it in `fire`. The verification review's fixes, the same day, added
about 2 ad and 1.5 days of path: E1's core rows, stale references, sentinels and ruin reserve in
`ids`, the two scenarios `fire` could not commit moved to `atk`, the iterated fall, and the smaller
corrections of `base`, `site` and `vfy2`. The cut order of section 4.5, now two cuts, takes about
3.5 days off the path, the same as before, because the cut the owner removed, Survey's refresh, was
off the path; it leaves the stage with no planned cut that only buys review time. Decision 1's
second option, not taken, would have taken about 8.

Plan units are not calendar days. S1's 71.5-ad plan ran from its adoption on 2026-09-29 to its close
on 2026-10-16. Its stops — the Windows CI leg, serial contract PRs, `MATRIX_ONLY`'s second matrix
for every lane touching `crates/sim/`, `crates/client-gdext/`, `godot/`, `crates/mesher/`,
`library/`, `rules/`, `.github/` or the chain goldens (`scripts/merge-train.sh`'s `MATRIX_ONLY`) — set the
calendar again: about 20 of S2's 27 PRs pay it, and up to 23 if `vfy1`, `vfy2` and `wgw` move a
fixture under `godot/`.

### 4.4 How lanes run in S2

- **`wave-lanes.js` is pointed at this plan** by `args.plan` = `docs/design/s2-plan.md` and
  `args.stage` = 'S2'; the task headings carry their ids in backticks ("### `base` —", "### `oprd` →
  `opra` —"), as S1's did. The environment's sentence on `cargo xtask determinism --bless` names the
  lanes whose section says the determinism chain moves (`ids`, `fire`, `tune2`).
- **The bulk ruling no longer gates a launch:** it fell on 2026-10-16, before wave 1 (item 144
  (11)), so every lane launches when its inputs have merged, and each lane's brief carries the
  rulings its Needs line names.
- **The main session's docs PRs** carry, beside the rulings: spec §3's "terrain … credits nobody" amended by item 144 (2) and decision 37; spec
  §6's Build row (terraform's meaning, the base kit's blueprints, repair drones on work in hand,
  decision 38's threshold), §3's fog-policy line (per-seat detail at match end only, the owner's
  ruling of decisions-log item 145 (7)), §6's Common row (decision 4's unset `roe`), §7 (decision 43's repair
  price under the Repair band), §9 (the support rule, materials and terrain's blast levels by
  material, decision 10, bedrock, walls at least two high and filled to the ground and the charge's
  cut, decisions 39 to 42, the demolition charge's carrier, and that a collapse carrying an asset
  does not happen in ordinary play, decision 39), §14 (decision 35: Easy's Defend guard moves into
  S2), §17's S2 row (the cuts taken, and Survey's refresh never among them, item 144 (11)), §19
  (the kingmaking check read from `tune2`'s three-seat run for S2, decision 36); AGENTS.md §3 rule
  4's new client pieces (`rnds`' `--rounds` reader, `view`'s), §9 item 11 (G3′'s alarm job and
  the certification on the owner's machine), §10 item 5 (the bands read against Easy until S5);
  `docs/placeholders.md`'s S2 rows; `docs/design/README.md`'s row for this plan. AGENTS.md is a
  harness doc no lane edits.
- **The spec's worked example does not move** (§10): it stays at [96, 11, 62], as
  `examples/playbooks/expand_east.jsonc` does, and `expand-east-segment` has its own playbook
  (decisions-log item 145 (2)).
- **Packaged locally before merging** (S1's decision 9): `redate` (`rules/README.md` ships), `con3`,
  `rnds`, `view`, `oprd`, `opra`, `tune2`, `night` (its named place in `xtask/src/package.rs`),
  and any lane given a named place under `godot/`, `library/` or `rules/` (`site`'s
  `rules/README.md` sentence if cut 2 is taken).

### 4.5 The cut order, written before the stage starts (item 144 (7), ruled by (11))

If S2 runs late, these two move to S3, in this order, and nothing else does without the owner's
word. **Survey's refresh pass is never cut** (item 144 (11)): the plan's third cut, re-checking the
sightings closest to leaving the reach window before the next Lull, left the order on the owner's
word, after the owner's line on decision 5 that intelligence and surveillance should be a huge
factor in the game, so full Survey ships in S2 whatever happens and item 13's refresh is not
deferred. A cut construct is never silently ignored: the sim answers `NotAtThisStage("S3")` and the
verifier its E-code (E8), so a playbook that uses it is refused with a reason and the rules text
says "from S3". Cut 2 removes behaviour rather than a construct, so nothing is refused for it, and
the player learns of it from the verifier's I-level note on a Build beacon whose `terraform` is
`FILL` or `BOTH`, that the crater refill is from S3, and from the same sentence in the rules text
(`site`'s brief carries both if the cut is taken).

1. **Attack's waves.** One launch per segment remains; `waves` greater than one is refused. It saves
   about 1.5 ad and 1.5 days on the critical path (`atk`). It goes first because it is not named in
   the S2 row, no other S2 feature reads it, and none of the three nightly arms needs a second wave
   (Rusher's "massed rush" is one launch).
2. **Terraform's crater refill** (`FILL` and `BOTH` refilling craters and trenches inside the
   sphere). Footprint levelling stays, so Build still builds on cratered ground. It saves about 2 ad
   and 2 days on the path (`site`). It goes second because it is a §2.7 addition (item 144 (5))
   rather than a named row item, and its absence leaves a moat to be undone by mortar, charge,
   breach or a footprint's `DIG`; it goes no earlier because it is the spec's own counter to a moat
   (§9: "Build's fill and dig are what undo it"), and the Turtle arm's band is the place a missing
   counter would show.

The two cuts together take about 3.5 days off the critical path and 3.5 ad. Without the third, a
slip past them meets no planned cut that only buys review time: it is the owner's to rule.

**Checkpoints** (decision 32), each tied to the start of the lane whose work a cut removes, so a cut
is taken before that work begins; "late" is the lane's start day against its planned start in
section 4.1:

- **At `site`'s start** (planned day 26, when `fire` merges): later than day 27, cut 1 is taken and
  `atk`'s brief drops waves; later than day 28.5, cut 2 as well, and `site`'s brief drops the refill
  and carries the note in its place. Later than day 29.5 (3.5 days late, more than both cuts
  absorb), both are taken and the main session also puts the slip to the owner.
- **At `atk`'s and `surv`'s start** (planned day 41.5, when `dfnd` merges): later than day 42.5,
  cut 1 if it was not taken. Cut 2's work is done by then, since `site` has merged, and Survey's
  refresh is never cut, so cut 1 is the only cut left here, and it absorbs 1.5 days: later than day
  43 with cut 1 spent, whether at this checkpoint or the first, no planned cut is left, and the main
  session puts the slip to the owner; `surv`'s brief keeps the refresh pass whatever the answer.

Beyond the second cut, any further cut is the owner's (decision 32, section 7).

### 4.6 The contract pull requests, in merge order

Nineteen of S2's PRs touch an AGENTS.md §5 path: seventeen lanes' and the main session's
harness and firewall-runner PRs. The main session's docs PRs (section 4.4), which touch AGENTS.md and
`docs/design/`, contract paths too, are counted apart: they are the record's own PRs, take the
docs-only fast path and queue on no lane. Each body names its paths; each new hashed field goes into
the state hash, the snapshot round-trip and the goldens in the same PR (§4.8); each snapshot change
bumps `SNAPSHOT_VERSION` (9 today, `crates/sim/src/snapshot.rs` ~153) and refuses an older snapshot
with a clear message. `SAVE_FORMAT_VERSION` (2, `crates/gateway/src/save.rs` ~99) moves only if a
lane changes the save container itself, which none is planned to; a save holding an older snapshot
is refused through the snapshot's own version. Event kinds and step failures are added at the end
with new names, their ids taken at the merge of `main` (E17, E33).

| Order | PR | Contract paths |
|---|---|---|
| 1 | `msrv` | `clippy.toml`, the workspace manifest's `[workspace.package]`, `xtask/` where the fix pass edits it |
| 2 | `con3` | `proto/gp/v1/rules.proto` and the generated tree, comments in `playbook.proto` and `gateway.proto`, `GetEconomyForecastResponse` 5 and 6 `optional`, `ResolvedRef.reachable`, `rules/rules.v1.json` (`rules_hash`), golden contents |
| 3 | `base` | determinism code: the tick loop's per-phase form and its `MidTick` refusal (E30); the event vocabulary (`StepFailure` `targets_unwritten`) |
| 4 | `con4` | `playbook.proto` (the header's defaults list, decision 4), `gateway.proto`, `proto/buf.yaml`'s scoped `ignore_only`, the generated tree, the schema and docs goldens |
| 5 | `night` | `xtask/src/scenario.rs` (what the `scenario` step checks), `xtask/src/package.rs`'s replay list (a named place: `collect_played`, so the package never replays an arm), `.github/workflows/nightly-scenarios.yml`, the nightly result golden (a new golden format) |
| — | the harness PR (main session, after this plan's item 145 docs PR; **merged as #122, `b1380aa`**, decisions-log item 146 (1)) | AGENTS.md §6 (the merge-only rule in place of "rebase on `main`", and `tests/golden/scenarios/nightly-result/` in the list of tests that pin the operator's output), CLAUDE.md (~76, the local-verify path), `scripts/merge-train.sh` (~19–21, ~53, ~188: it rebase-merged, rebased the next lane with `--force-with-lease`, and refused a branch with merge commits), and `.claude/workflows/wave-lanes.js` (~106) (decisions-log item 145 (3) and (7)). The workspace `[lints]` half is its own later contract PR, the next row. Since it merged, `scripts/merge-train.sh` is in use again, merge-only: every PR merges by `gh pr merge --merge --match-head-commit`, and a lane takes `main` by a signed merge |
| — | the lints PR (main session, after the harness PR) | the workspace `[lints]`: `arithmetic_side_effects` and `panic` denied in the crates that parse untrusted input, and `todo` and `unimplemented` at the integration gate (decisions-log item 145 (5); split from the harness PR by item 146 (3) (c)) |
| — | the firewall-runner PR (main session) | `.github/workflows/claude.yml`'s `runs-on` (item 142 (3)), and `ci.yml`'s DCO message (~390), which still says `git rebase --signoff <base>`, brought to AGENTS.md §8's remedy (item 146 (4)); in the gap between `night` and `g3h`, once the owner's first triggered `claude.yml` run has happened |
| 6 | `ids` | determinism code: ids and generations, the typed id columns, the placement counter, the beacons-lost column, the ruin reserve, `SNAPSHOT_VERSION` 10; the event vocabulary (`ruin_buried`) |
| 7 | `vfy2` | `report_hash`'s `Scope` encoding |
| 8 | `fire` | the combat phase, the kill-credit split (E32), `Stream::Combat`'s first draw, the shell table, wall, cracked terrain and bedrock materials (new wire values in the chunk digests; bedrock written by generation, moving the mapgen digest goldens), the destroyed-value and tally columns, the attacker memory, `SNAPSHOT_VERSION` 11, event kinds, the bore's golden (a new golden format) |
| 9 | `fall` | the Voxels phase's order and its settle (E2); event kinds |
| 10 | `site` | drone state and the snapshot if it adds any; the generated-surface golden (a new golden format); event kinds |
| 11 | `g3h` | `xtask/src/main.rs`' perf-alarm list |
| 12 | `dfnd` | Defend's table and the explored table, their hashing (E29), the snapshot; event kinds |
| 13 | `oprd` | `tests/golden/package/`'s manifests |
| 14 | `atk` | Attack's table, the snapshot; event kinds; `allocations.rs`' crowd |
| 15 | `opra` | `tests/golden/package/`'s manifests |
| 16 | `g3r` | `cargo xtask g3-budget`, `.github/workflows/ci.yml`'s new job |
| 17 | `tune2` | the rules' values (`rules_hash`), the scenario format (the walkers' key removed), `xtask/src/scenario.rs` |

As merged so far: `msrv`, `con3`, `base`, `night` (decisions-log item 145 (1)), then `con4` #120,
the harness PR #122, `ids` #123 and `vfy2` #125 (decisions-log item 146 (1)).

`NIGHTLY_SCENARIOS_ENABLED` is not a PR: it is a repository variable the owner sets (decision 30).

---

## 5. G3′-real, the depth task

**The criterion, verbatim from spec §16's G3′ row.** When: "Synthetic tick check in the spike; the
real measurement is S2's exit; scrub at S7". Go if: "4 seats (headroom over v1's 3), 300 units, 40
beacons: tick p99 ≤50% of the tick; ≥4× speed on the headless sim runner. The measurement sets the
per-map power budget for the generator. Scrub ≤1 s is judged at S7". Fallback: "Lower map power
budget; shorter segments". Item 144 (8) names it S2's depth task (AGENTS.md §10 item 6).

**The load generator.** Real play cannot reach the gate load: at `map_ceiling_kw` 190 and
`kw_per_unit` 1 the generator powers 190 units, and 40 beacons placed by walk-in, the commander
walking one voxel a second, do not fit one segment. So the harness drives E30's load generator: a
**bench rules variant**, built in memory from `rules/rules.v1.json` with named overrides (the power
ceiling raised to the gate load; the fielding totals are world constants already at 300 units and
40 beacons, E30), its own `rules_hash` printed in every
report and never shipped; and the **gate's layout**, 40 beacons with their mandates (Build, Defend,
Attack, Mine and Survey in the proportions the report states) and their starting units, placed once
through `Host::seed_layout` in the lobby, so the layout is an input like the seed and the run
replays. From there the match is real play: the mandates fabricate, fight, crater, collapse and
repath on their own. The harness walkers take no part; S2-16 removes them after the measurement
(`tune2`, decision 23).

**What S2 certifies:**

1. **G3′-a, the tick's p99 ≤ 25 ms, and G3′-b, at least 4× real time on the headless runner (a mean
   ≤ 12.5 ms, ≥ 80 ticks a second),** at 300 units and 40 beacons as world totals (item 63) with
   decision 2's seats, over a full 8-minute segment of real play: combat, the support rule, the
   pathing the destruction causes (no charged substitute, the spike's 96.5 % caveat retired; CSR
   counted once, item 64), and a crater stream at the destruction rate the budget is set with.
2. **The per-phase figures the budget needs:** the tick-minus-pathing mean (item 116 (6) (b)); the
   repath backlog against the cap and the measured burst frequency, from which the cap is re-derived
   (item 69); the broadphase's share with E11's pad and its growth with density (item 67's caveat),
   which sets the cell size; the support search's adversarial tick (many seeds, several digs, a
   region of `max_region_voxels`) and the per-tick visit budget's use (item 138 (3)); kill-credit
   settlement at the run's death rate (G3′ §9.12); live heap bytes over the segment and the tick p99
   per minute, for drift (item 68; the spike's step 6); allocations per tick with a moving crowd
   (`atk`).
3. **The per-map power budget**, re-derived as item 65's arithmetic did, every input now a
   measurement, with `power.kw_per_unit`, `reserve_percent` and `map_ceiling_kw` moving together
   (item 91) and the destruction rate recorded in the same decision; the fielding totals (S1-37,
   S2-08) and the walkers (S2-16) with it.
4. **Recorded runs on the owner's demo machine (i7-9800X) under Windows and under WSL2 Linux**,
   three repetitions each, the median of the p99s, the clocks before and after; **the slower OS sets
   the figure** (the spike's step 7: both are first-class). The three CI runners' figures are
   published beside them as notices, and never decide the gate (E18).
5. **Re-certified on `tune2`'s numbers.** `tune2` moves the power budget and the rows the gate load
   reads, so `demo2` re-runs item 4's runs on the final rules and records them; the gate is the
   re-run's figure.

**The CI job is an alarm, not the gate.** `g3 budget (ubuntu-24.04)` runs the same load on a hosted
runner and fails only at the gate's numbers doubled (a p99 over the whole 50 ms tick or a mean over
25 ms), so it catches a regression that doubles the tick and does not flap on a shared runner's
noise (E18; decision 31).

**The fallback, written now, lever by lever.** If G3′-a or G3′-b misses at the gate load, the
spec's fallback is taken in writing, with the owner's word, and each lever answers a different miss:

- **A per-tick miss** (the p99 or the mean over budget from the first minute): **a lower map power
  budget.** Fewer `kW` powers fewer units, so the load the budget implies falls. The new
  `map_ceiling_kw`, with `kw_per_unit` and `reserve_percent` moving with it (item 91), sets a lower
  unit total; the harness re-runs at **that** load, and the gate is re-certified there and recorded
  with the load it was met at. Re-running at the old 300 units with a lower budget would measure
  nothing the budget changed.
- **A drift miss** (the tick p99 rising from minute 1 to minute 8, or live heap growing over the
  segment): **shorter segments**, a ladder below 3 / 5 / 8, which bounds what a segment accumulates
  (copy-on-write chunk growth, wrecks) and interacts with the fun gate's own fallback (item 68).
  Shorter segments do not lower a per-tick figure, so they are not the answer to the first miss.
- **Neither lever is the repath cap**, which is item 69's and is re-derived in either case, nor
  `max_region_voxels` or `max_visits_per_tick`, which `g3r` lowers with their in-world reason if the
  support search's worst tick alone breaks the p99 (R4).

---

## 6. Owner decisions before the stage starts

Recommendation first, marked **(Recommended)**, downsides for each option. **[play]** marks the
play-shaping ones, put first; **[risk]** marks a risk the owner accepts or refuses. Hygiene and
engineering are not asked here; they are section 2.3's calls. Decisions 35 to 43 came from the two
reviews of this plan. Numbers 1 to 34 are kept as they were so that every reference to them holds;
decisions 20, 24 and 27 turned out to be engineering and stand as E26, E27 and E28, their numbers
left as pointers. **All ruled on 2026-10-16 (item 144 (11)),** before wave 1: each decision says how,
and keeps its options for the record. The owner was interviewed on decisions 1, 4, 5, 7, 9, 35, 37,
40 with 10, 41 and 43 and took the recommendation on each but 10 and 40; every other decision passed
as recommended in the bulk answer. A lane that finds one of them wrong in the code raises it with
the owner rather than building around it.

**1. S2's size and schedule.** *Governs everything.* **Ruled:** the recommended option, put to the
owner in the interview (item 144 (11)); with decision 10's ruling the size was about 126 ad and
still about 13 plan-weeks, and with the verification review's engineering fixes it is about 128 ad
and 13.5 plan-weeks (section 4.1), the half-week the review added being engineering the ruling
already covers.
- **(Recommended) Keep the whole row and let S2 run about 13 plan-weeks,** with section 4.5's cuts
  as the only planned reductions, taken at their checkpoints. *Downside:* later rows shift about
  seven plan-weeks unless absorbed; the gate date is the owner's.
- *Two sim lanes: `site` beside `fire` as a named place* (Build's mandate and programs beside the
  combat phase), about 8 days off the path. *Downside:* two agents in the determinism crate at
  once, both bumping the snapshot and touching `tables.rs`, where a bad merge is a desync; S1's
  decision 1 refused the same trade.
- *Take the cuts now:* about 3.5 days off. *Downside:* the stage ships less than the row and a moat
  lasts. (As put to the owner this option took three cuts, Survey's refresh among them; the owner
  has since ruled that one never cut, section 4.5.)

**2. [risk] G3′'s seat count.** *Gates `g3h`.* The spec says "4 seats (headroom over v1's 3)"; v1
has at most three, the generator places three zones and the gateway refuses a fourth (E19).
**Ruled:** the recommended option, in the bulk answer, the owner accepting the risk (item 144 (11)).
- **(Recommended) Measure at three seats with the world totals (100 units a seat), and report a
  fourth, unplaced seat's decision-tick cost as a separate line.** Item 63's world totals already
  give each of three seats more load than four seats would (75 each). *Downside:* the spec's literal
  "4 seats" is not run; its headroom is argued, not measured, and the owner accepts that risk.
- *Four placed seats on a four-zone test map:* *Downside:* a generator change for a configuration v1
  cannot play, and a gateway path no shipped build reaches.

**3. [play] Combat's place in the tick, and how a shell aims.** Section 2.3's E2 builds it as
engineering (every damage in the tick it is dealt); it is named here because it shapes what a
player sees: fire is simultaneous, so two assets that kill each other on one tick both die; and a
mortar shell lands where its target stood when it was fired, with no lead (E13), so a unit that
keeps walking dodges it. **(Recommended)** both, as E2 and E13. *Downside:* no "first shot wins",
and a mortar is weak against anything that moves. *Alternatives:* fire resolves in seat order.
*Downside:* the lowest seat wins every exact duel. A leading shell that aims at where a moving
target will be. *Downside:* the sim models enemy movement for aim, and the spec's "slow arcing
shell that can be dodged" stops being true. **Ruled:** the recommended option, in the bulk answer
(item 144 (11)).

**4. [play] An unset `roe`.** `playbook.proto`'s header makes unset an error and allows an
owner-ruled exception list of four omitted settings; nothing enforces it for `roe`, and Hold &
Build, the safe playbook and Easy's output all omit it. **Ruled:** the recommended option, put to
the owner in the interview (item 144 (11)): unset reads as `RETURN_FIRE`, the fifth omitted-setting
default. `con4` writes the header's entry and `vfy2` builds the I-level note, extending `I0003`, the
note for an omitted setting and its named default.
- **(Recommended) Unset reads as `RETURN_FIRE`, a fifth entry on the list, with an I-level note,
  which `con4` writes into the header (E25).** The spec's Common row says return fire and retreat
  "follow the rules of engagement and are always announced", and its worked example writes
  `RETURN_FIRE`. The three values read as: `HOLD_FIRE` never fires; `RETURN_FIRE` fires only at an
  enemy asset that damaged one of the seat's own assets inside the firer's range within the
  under-attack window; `FREE_FIRE` fires at any enemy in range. The core's autocannon, which has no
  mandate of its own, fires as `FREE_FIRE` (decision 8). A unit below `retreat_hp_pct` walks to the
  nearest own beacon whose sphere holds a live repair drone, else to its home beacon, firing only as
  its `roe` allows (Defend's "fall back to the nearest friendly repair drone", spec §6, made
  common). No committed chain moves for this alone, since nothing is armed in a committed scenario
  before `fire`'s; the chains `fire` moves, it moves for its new columns. *Downside:* a fifth
  invented default, irreversible once files omit it.
- *Unset is an error (E-code):* *Downside:* every template, the safe playbook, Easy and the
  determinism playbook must write it, and every operator and `demo_*` golden moves.
- *Unset reads as `HOLD_FIRE`:* *Downside:* a seat that never wrote a `roe` never shoots back.

**5. [play] S2-11: what a seat sees of an enemy asset.** **Ruled:** the recommended option, put to
the owner in the interview (item 144 (11)). The owner added: "I want intelligence and surveillance
to be a huge factor in this game." The line is why Survey's refresh pass is never cut (section 4.5),
and it is the owner's to apply to later stages' sensing (pickets, the Sensor Spire, the reach
filters); this plan adds no rule for it beyond the ruling.
- **(Recommended) In live vision now: the asset's kind, position, heading and exact HP; from memory:
  kind, position and the sighting's age, nothing else; a beacon's mandate, power state, units by
  role and build progress are never shown.** Attack's "weak point" and `weakest` read HP, and a
  shell or a wreck is plain to see. `get_beacon`'s prose follows (E9). *Downside:* HP is perfect
  information for what you see; perfect-intel snowballing is a spec §6 risk.
- *HP in bands (full, damaged, critical):* *Downside:* a second representation the selectors must
  rank, and a rule nobody stated.
- *Position only:* *Downside:* "weak point" and `weakest` cannot be computed for an enemy.

**6. [play] S2-02: the safe playbook's rescue rule.** Spec §14 says "Otherwise shadow the safest
beacon, with the flee and rescue rules" and states no rescue; spec §10's reflex, at the commander's
HP ≤ 20 %, "walks the commander to the safest own beacon, and then the route continues from the step
it was on". **Ruled:** the recommended option, in the bulk answer (item 144 (11)).
- **(Recommended) Rescue rides `safest`, with no new vocabulary: while the commander is below full
  HP, `safest` ranks own beacons whose sphere holds a live repair drone ahead of those without, then
  by decision 12's threat-adjusted rank.** The reflex, the safe playbook's shadowing of the safest
  beacon and Easy all read `safest`, so each walks a wounded commander to a beacon where a drone
  heals it (`site`), and the flee rule and the safe playbook's "never recycles, switches or places"
  are unchanged; otherwise the safe playbook stays as spec §14 writes it under combat, its omitted
  `roe` reading per decision 4. `dfnd` builds the term; `oprd` and `wgw` only follow it. *Downside:*
  an invented reading of one word; after the reflex the route continues, so a commander whose
  playbook walks on leaves the drone before it is healed.
- *A playbook rescue construct* (a selector "own beacon whose sphere holds a live repair drone" and
  a step that holds until HP is full): *Downside:* new vocabulary in `playbook.proto` (`con4`), a
  selector and a hold in the sim, a check in the verifier (`vfy2`) and the operator's use of both,
  for one rule.
- *Defer to S5 with Normal:* *Downside:* the safe playbook stays flee-only through the stage that
  makes it matter.

**7. [play] X-08 and the destroyed-value term.** The score is held value plus enemy value destroyed
(spec §3). `fixs` read the first tie-break, "unsmoothed net worth", as that score (`audit.rs`
~43–53), and the code's own note says the two readings agree only while destroyed value is zero and
rank score-tied seats in opposite directions once it is not. **Ruled:** the recommended option, put
to the owner in the interview (item 144 (11)).
- **(Recommended) "Unsmoothed net worth" is held value alone — the Ledger's raw figure,
  `World::held_value`, without the destroyed term — so among score-tied seats the one holding more
  wins, then the one that destroyed more, then fewer beacons lost, then a shared win; "enemy value
  destroyed" is each seat's settled share of the build cost of every enemy asset it killed,
  accumulated as E15 states, recycling booked as destruction is.** It gives the tie-break a meaning
  the score does not already carry, and "unsmoothed" then contrasts with the Ledger's ranked
  standing, as the word suggests. No golden moves for it before `fire`, since destroyed value is
  zero everywhere until then. *Downside:* it reverses `fixs`'s reading, and between score-tied seats
  the builder beats the raider.
- *Keep `fixs`'s reading (the score):* *Downside:* the first tie-break repeats the score, so enemy
  value destroyed decides every tie and the destroyer beats the builder.

**8. [play] What the core's autocannon fires at.** Spec §4: "so a single early raid can't eliminate
a seat". **Ruled:** the recommended option, in the bulk answer (item 144 (11)).
- **(Recommended) Enemy units only (the enemy commander included), nearest first.** *Downside:* an
  enemy mortar or wall built in its range is ignored; the raider is the mortar's counter (spec §9).
- *Units and structures:* *Downside:* the core out-trades a siege by itself.

**9. [play] Who carries a demolition charge.** Spec §9 names only "the carrier"; the commander
carrying one is roadmap (§4). **Ruled:** the recommended option, put to the owner in the interview
(item 144 (11)).
- **(Recommended) A raider of an Attack beacon with `breach_allowed`, loaded at `$` 15 when the
  breach needs it, planting at the breach point; a carrier killed before it plants drops a charge
  that never fires.** *Downside:* the raider's role widens; a charge cannot be placed by Build.
- *A build drone of the Attack beacon:* *Downside:* Attack beacons field drones only for this.
- *A new unit kind:* *Downside:* a sixth unit kind, which `rules/README.md` says does not exist, and
  its rows.

**10. [play] Walls and materials.** Spec: a wall is "cheap blocking terrain", priced per voxel, "HP
is by material" (`rules.proto` ~666). **Ruled: per-material hardness, the owner's choice against the
recommendation** (item 144 (11), put to the owner in the interview with decision 40). The ruled
form, which every lane builds:
- **Walls.** A wall target is a box (`Area`) of voxels filled above the ground, at least
  `materials.wall_min_height_voxels` high (decision 40), each voxel at `wall_cost_per_voxel`. A wall
  voxel has two levels, whole and cracked: it cracks on its first blast and breaks on its second.
  Wall material carries its seat; a wall outside every live sphere of its seat reads as neutral,
  which no material id says (E14, decision 42). A wall is filled down to the ground (decision 39).
- **Terrain resists a blast by material too.** `materials.blast_levels` gives each terrain kind its
  number of levels N, and a voxel of that kind takes N blasts: each blast inside a crater's radius
  steps it down one level, as walls do, and the blast at its last level removes it. The damage
  level is held in the material id, as E14 does for walls, so it lives in the voxels the chunk
  digests already hash and no table is added; the catalogue holds up to three levels a kind, and a
  row outside 1 to 3 is refused at load (E14 says what the sim, the codec, the wire table, the
  client and the mesher each need). **Proposed** (a `con3` row, owner at S2's demo, with `tune2`'s
  duel report): dirt 1, ore 1, vent 1, stone 2. A mortar shell therefore clears the dirt skin and
  any ore or vent at once and cracks the stone beneath, and a second shell on the same ground digs
  on, so a crater in rock costs two shells.
- **Shapes.** Mortar craters stay balls, stepping each voxel inside down a level. A demolition
  charge cuts flat down to the wall's footing and removes every voxel inside its cut whatever its
  level, so its gap is walkable (decision 40; a cut that left cracked stone standing would leave a
  step).
- **Never removed:** bedrock (the bottom layer) and the core's bore (E31).
- **Nothing restores cracked terrain in S2.** Repair restores own wall voxels only (decision 42),
  and terraform's refill fills air, so a cracked voxel stays cracked.
- *Downside, the owner's to accept:* more rows and a tuning surface before the duel report exists,
  about 1.75 ad (`fire` 1–1.5, `con3` and `view` a quarter each) and 1.25 days of the critical path
  through `fire`; a crater in stone is shallower than one in dirt, which the duel report's mortar
  lines show.

*For the record, the options as put to the owner:*
- *(Recommended, not taken) A wall target is a box (`Area`) of voxels filled above the ground, each
  at `wall_cost_per_voxel`; a wall voxel has two levels (whole, cracked) and breaks on a blast at
  the second; any blast removes dirt, stone, ore and vent voxels inside its crater radius; the
  bottom layer is bedrock and the core's bore is never removed; wall material carries its seat,
  with a neutral wall at each level (E14, decision 42).* *Downside:* stone is no harder than dirt.
- *(Taken, with the walls of the first option) Per-material blast resistance for terrain too:*
  *Downside:* more rows and a tuning surface before the duel report exists.

**11. [play] Collapse's constants, and an asset under a landing region.** Item 138 (3) asks for the
constants as proposed rows. **Ruled:** the recommended option, in the bulk answer (item 144 (11));
the values stay proposals, the owner's at S2's demo.
- **(Recommended) `collapse.max_region_voxels` = 4 096, `collapse.max_visits_per_tick` = 16 384
  (four regions' worth) and `collapse.fall_damage_hp` = 100 (all proposals, owner at S2's demo); an
  asset under the voxels a region lands on takes the same flat damage and stands on the new top.**
  *Downside:* a region of 4 097 voxels never falls, and under a heavy bombardment the tick budget
  can leave standing a region that should fall, either of which a player may notice; 100 HP kills
  every drone (100 HP) and every scout (60), leaves a raider (120) at 20 and the commander (300) and
  every structure standing, so item 144 (1)'s downside, "one crater under a beacon can kill it",
  holds at this value only for units, until the owner tunes it.
- *An asset under a landing region is destroyed:* *Downside:* a second damage rule, unbounded.

**12. [play] The threat model.** Spec §6 names threat-weighted targeting, `most_threatened`, "avoid
known threats" and SAFEST without a formula. **Ruled:** the recommended option, in the bulk answer
(item 144 (11)).
- **(Recommended) An enemy asset's threat is its weapon's damage per second (rows), zero if unarmed;
  `most_threatened` is the own beacon with the largest summed threat of enemy assets seen inside its
  sphere, ties to the lowest id; `beacon_under_attack` is the predicate's own window;
  threat-weighted targeting picks the highest threat inside the engagement radius, ties by distance
  then id; `AVOID_KNOWN_THREATS` adds the rules row's surcharge per step inside a known threat's
  range; `safest` subtracts the summed threat from its HP-fraction rank, after decision 6's repair
  term; `SAFEST` picks the nearest seam with no known threat within its range; `flee_on_threat`
  walks a mine drone home while a known threat is inside its beacon's sphere.** *Downside:* a model
  built from weapon rows only, blind to unarmed threats like a charge carrier until it is armed.

**13. [play] Defend's unstated settings.** Auto-fortify and unit mix have no stated meaning.
**Ruled:** the recommended option, in the bulk answer (item 144 (11)).
- **(Recommended) Defer both to S3's rule list, their numbers reserved, refused with E-codes; Defend
  fields raiders as guards and scouts as pickets on work in hand.** *Downside:* Defend reads thinner
  than the spec's row.
- *Define them now* (auto-fortify: the beacon queues an autocannon when a threat is seen; unit mix:
  the raiders' share of guards): *Downside:* two invented rules.

**14. [play] Attack's settings' values.** The spec names the settings, not their options.
**Ruled:** the recommended option, in the bulk answer (item 144 (11)).
- **(Recommended) Retarget: `NEAREST_KNOWN` or `NONE`; after-action: `REGROUP_AT_STAGING` or
  `RETURN_HOME`; retreat: below a percentage of the launch force, or after a game time; weak point:
  the target beacon's lowest-HP structure inside its sphere, else the beacon; waves: launches per
  segment (cut 1).** *Downside:* a vocabulary written now, irreversible once files use it.

**15. [play] `BuildSettings` 6 and 7** (reserved as `repair_drones`, `reclaim_drones`). Spec §6:
"repair/reclaim drone fields", and "No fixed counts". **Ruled:** the recommended option, in the
bulk answer (item 144 (11)).
- **(Recommended) Both stay reserved; repair drones follow work in hand.** *Downside:* a player
  cannot turn salvage off.
- *Two counts:* *Downside:* contradicts "no fixed counts". *A job switch:* *Downside:* a sixth
  omitted-setting default.

**16. `report_kinds`** (`MandateSettings` 3). **(Recommended)** S3, with `get_reports`, which
answers `INTERNAL` today; S2's announcements ride the feed. *Downside:* a beacon cannot filter its
reports. **Ruled:** the recommended option, in the bulk answer (item 144 (11)).

**17. [play] Combat's numbers, and how they are set.** The spec gives HP, costs, `kW` and speeds
(item 90) and no damage, range or rate. **(Recommended)** rows proposed from the kit's counter
relations, all `PLACEHOLDER: tuning — owner, at S2's demo`, checked by `tune2`'s duel and balance
reports, tuned at the demo. **Ruled:** the recommended option, in the bulk answer (item 144 (11));
the table gains decision 10's blast levels, and every number in it stays a proposal:

| Row | Proposal |
|---|---|
| autocannon | range 20, 12 damage, 250 ms (48 a second: a raider in 2.5 s), vision 20 |
| the core's autocannon | range 24, 12 damage, 250 ms |
| mortar | range 40, 60 damage flat within a splash radius of 3 (no falloff), crater 2, 4 000 ms, shells at 8 voxels a second (5 s at full range; a raider moves 6 voxels in that time) |
| raider gun | range 4, 8 damage, 500 ms (a mortar in 25 s alone), vision 12 |
| vision | scout 24, commander 10, drones 8, mortar 12, Generator and survey post 8 |
| demolition charge | radius 4, fuse 5 000 ms |
| repair | 20 HP a second |
| terraform | 1 000 ms a voxel |
| threat surcharge | ×4 step cost inside a known threat's range |
| Defend's under-attack window | 10 000 ms |
| picket radius | 6 voxels beyond the sphere edge (spec §19) |
| terrain blast levels (`materials.blast_levels`) | dirt 1, ore 1, vent 1, stone 2, each from 1 to the catalogue's 3 (decision 10); a wall's two levels are the ruling's, not a row |
| the collapse rows and the wall's minimum height | decisions 11 and 40 |
| Hold & Build's repair threshold | 50 % (decision 38) |

*Downside:* the numbers are as good as the duel report and Easy's play.

**18. The alarm bands' wording.** **Ruled:** the recommended option, in the bulk answer (item 144
(11)). **(Recommended)** "Rusher (Turtle, Hunter): an arm that is the
sole winner — last seat standing, or the final audit's — in more than 13 of the 20 seeds (65 %)
against Easy opens a tuning task. At S2 the band reads against Easy, which carries the Balanced
weighting (item 144 (6)); it is re-baselined against Normal at S5." *Downside:* a shared win counts
as no win, which favours not alarming.

**19 to 28. The register's owner rows not decided above.** **Ruled:** each as recommended or as
written, in the bulk answer (item 144 (11)); the owner vetoed none of the engineering calls named
here.
- **19. S2-03 and S2-04** are decision 12; **S2-01 and M-01** are `night`'s shape with decisions 18
  and 29 (Easy the one operator, item 144 (6)); **S2-07 and S2-09** are engineering calls E5 and
  E11, listed here for the owner's veto.
- **20.** Now **E26** (S2-05's row; engineering).
- **21. [play] The raider's target** (spec: "hits the nearest structure"). **(Recommended)** the
  nearest enemy structure or beacon by default; a raider guarding under Defend engages enemy units
  inside the engagement radius; under `RETURN_FIRE` it answers the unit that hit it. *Downside:*
  Defend widens the raider's stated role. **Ruled** as recommended (bulk).
- **22. S1-34** becomes per-kind vision rows (decision 17's values). **Ruled** as written (bulk).
- **23. S1-37** stands at 40 / 300 / 120 world totals, now counting live assets (E1), re-derived by
  `g3r`; **S2-16**: the walkers and their 256 cap are removed from the scenario format and the sim
  by `tune2`, after G3′-real. G3′'s load comes from E30's layout seam and bench rules variant, never
  from the walkers, so nothing the gate measured depends on them. *Downside:* a scenario can no
  longer add load by key; a load test drives a layout through the bench instead. **Ruled** as
  written (bulk).
- **24.** Now **E27** (S2-06, `MAX_ROUTE_NODES`; engineering).
- **25. [play] S2-08, the structure and wreck tables.** **(Recommended)** the wreck table's room
  equals the unit and structure rooms together, so every live asset can leave one; a wreck that
  finds it full replaces the oldest unsalvaged wreck, counted and announced (a proposed in-world
  reason: the ash buries old wrecks). *Downside:* a rule, and wrecks that expire. **Ruled** as
  recommended (bulk).
- **26. [play] S2-10, a dropped paid target.** **(Recommended)** removing a paid target from the
  list recycles what stands for it, refunding 50 % of its remaining value, as recycling does (spec
  §7). *Downside:* an edit can lose money. *Alternative:* it stays, finished never. *Downside:* a
  stranded asset. **Ruled** as recommended (bulk).
- **27.** Now **E28** (S2-12, `get_view`'s delta mode, decided by measurement; engineering).
- **28. S2-13** is decisions 13 and 14.

**29. [play] The nightlies' seat layout and "winning".** **(Recommended)** twenty seeds, each played
once, the arm alternating seats by the seed's position; the arm wins a seed when it is the sole
winner; `win_rate_percent` = wins × 100 / seeds, rounded down. *Downside:* the arm plays ten seeds
from each seat, not every seed from both. *Alternative:* both sides per seed (40 matches an arm).
*Downside:* "65 % of seeds" needs a new reading. **Ruled:** the recommended option, in the bulk
answer (item 144 (11)).

**35. [play] Easy's Defend guard on its commander: S2 or S5.** Spec §14: Easy "adds a Defend guard
on its commander when the commander has died recently". The skeleton's notes sent "the commander's
Defend guard" to S5 (`docs/design/skeleton-plan-w6-notes.md` ~418), and
`crates/operator/src/lib.rs` (~61) carries "Easy's Survey post, Defend guard, chatter and target
spread — owner, S5, with Normal and Hard". **Ruled:** the recommended option, put to the owner in
the interview (item 144 (11)): the guard moves into S2's `oprd`, which builds it.
- **(Recommended) Move the Defend guard into S2's `oprd`: Easy adds a Defend guard on its commander
  in the round after a Push in which its commander died** ("recently" read as the previous round's
  Push), `lib.rs`' marker re-dated for the three parts that stay S5's, and the main session's docs
  PR records the stage move beside spec §14. The Hunter arm's band (decision 18) measures whether
  hunting dominates, and against an Easy that never guards its commander that band measures nothing
  about balance. *Downside:* an operator behaviour lands three stages before Normal and Hard share
  it; `oprd` grows about half an agent-day; "recently" is a reading.
- *Keep it at S5:* drop it from `oprd` and from demo step 6. *Downside:* Hunter's band at S2 reads
  against a target that never defends, and Defend's `my_commander` (`dfnd`) ships with no built-in
  user.

**36. [risk] How S2 measures spec §19's kingmaking check.** Spec §19 checks "kingmaking and farming
the weakest seat" in "the nightly adversarial scenarios (share of damage aimed at the trailing
seat)"; the arms are two-seat (item 144 (6), decision 29), and with two seats there is no third seat
to aim at the trailing one. **Ruled:** the recommended option, in the bulk answer, the owner
accepting the risk (item 144 (11)); `tune2` builds the three-seat run.
- **(Recommended) Keep the arms two-seat; `tune2`'s balance check adds a three-seat headless run
  (three Easy seats, the twenty seeds) and reports the share of damage aimed at the seat trailing in
  the Ledger's standing when it was dealt, from `fire`'s unhashed counter (E15); for S2, spec §19's
  check is read there, and it is re-checked with three-seat arms at S5, when Normal and Hard give
  the arms other opponents.** *Downside:* the check reads Easy against Easy, not an adversarial arm,
  and has no alarm band, only a figure the owner reads; the risk that kingmaking pays goes
  unmeasured against a deliberate kingmaker until S5.
- *A three-seat arm in S2* (an arm and two Easy seats): *Downside:* `night`'s seat layout and its
  "winning" (decision 29) need a three-seat reading, `arms` grows by about a day, and a band for it
  is a new rule.

**37. [play] Collapse credit when several seats' edits cut one region.** Item 144 (2) gives the
credit to "the seat whose edit detached the region"; the plan's first reading picked the seat of
the region's lowest-index seed when more than one did. **Ruled:** the recommended option, put to
the owner in the interview (item 144 (11)).
- **(Recommended) Split the fall damage's credit equally among the distinct seats whose edits that
  tick removed a voxel next to the region (its seeds), as integers by largest remainder, ties to the
  lowest seat (spec §3's integer rule); the asset's own seat's share and a seat-less edit's credit
  nobody.** *Downside:* a seat that chipped one voxel shares equally with the seat whose charge did
  the work.
- *The seat of the region's lowest-index seed:* *Downside:* the credit is decided by voxel
  numbering, which no player can see.
- *Nobody, whenever two seats are involved:* *Downside:* it contradicts item 144 (2) in exactly the
  case a fight produces.

**38. [play] The repair threshold's reading.** `playbook.proto` says of `repair_threshold_pct`:
"Repair a structure of yours inside this beacon's sphere once it falls below this percentage of its
HP … 0 means never" (~1308–1310); the plan's first reading repaired regardless. **Ruled:** the
recommended option, in the bulk answer (item 144 (11)).
- **(Recommended) Honour the field as written: a drone repairs an own structure once its HP falls
  below `repair_threshold_pct` of its maximum, and 0 means never; own units and the commander are
  always repaired below full (spec §4); Hold & Build and Easy write a threshold of 50 (a proposal,
  owner at S2's demo), since every playbook on `main` omits the field and would otherwise never
  repair a structure; `vfy2` gives an I-level note where a Build beacon's threshold is 0; `con4`
  rewrites the comment and terraform's stale header sentence.** *Downside:* an omitted threshold
  means "never", which a player may not expect, softened only by the note; the templates' goldens
  move in `oprd`.
- *Repair regardless, the threshold only marking "damaged" for the build order:* *Downside:* it
  contradicts the field's own comment and leaves 0 with no meaning.

**39. [play] How often a collapse happens, and demo step 5.** Under the face-connected rule a wall
standing on the ground stays joined to its neighbours and the ground, so a charge at its footing
removes a ball and nothing above it falls; on a generated 2.5D map, whose columns are solid to
bedrock, a collapse needs something spanning a gap an attacker opened, such as a wall section whose
last footing is cut. **Ruled:** the recommended option, in the bulk answer, and again by name once the verification review showed that no ordinary play reaches a collapse: `fall` is built and the demo stages it (item 144 (11)).
**Applied, and its staging corrected** (operator item O1, adopted by the main session under the
owner's standing instruction "go with your recommendations"; the owner can veto): **walls fill
down to the ground**, so a wall box has no air under it (`site`), and a charge's flat cut, which
stops at the footing, never leaves anything above it unsupported: a breach never causes a
collapse. That removes the geometry the recommended option staged, a wall section whose only
footing the charge cuts, and the code leaves no other that play can reach with an asset on it. The
generated terrain is a 1-Lipschitz heightmap with no overhangs, every column solid from bedrock up
(`mapgen.rs`, "What it produces", 1), so there is no ridge, neck or shelf to cut; a mortar crater
is a ball centred where its target stood, on a column top, so it leaves the ground under and
beside it joined to bedrock; Mine never digs in or beside a structure's or beacon's column
(`mining.rs`, "never under structures") and never detaches a region carrying one (item 144 (3)).
The one free-standing terrain play can make is a Mine pillar (a `pillar_spacing` of 2 or more with
the pit dug three or more layers round it; Expand & Mine writes none), whose top a crater landing
beside it can drop, but which no unit can climb, so it carries nothing and its fall credits
nobody. So **demo step 5's collapse is staged, and the run sheet says so:** the run sheet runs
`fall`'s headless `collapse_credits_the_blast` beside the zip, whose terrain shelf, carrying a
structure, a unit and the commander, is built through test seams and cut by two mortar craters,
and shows its event log; a committed scenario cannot stage it, since the scenario format has no
placement key and `gamectl` no host seam. The plan and the run sheet say plainly that a collapse
carrying an asset does not happen in ordinary play until physics or a span limit.
- **(Recommended) No span or cantilever limit: keep the face-connected rule; the run sheet stages
  demo step 5's collapse on the golden seed (a wall section whose only footing the charge cuts), and
  `collapse-credits-the-blast` pins the same geometry headless; the plan and the run sheet say
  plainly that collapse is rare in ordinary play.** *Downside:* a feature the row names is mostly
  unseen in play until physics (item 138 (3)), and the demo shows a staged instance.
- *A span limit* (a voxel more than N columns from a grounded column falls): *Downside:* more
  collapses in play, at the cost of a second support term, a row, its worst case in G3′, and walls
  that break under their own weight.
- *Drop collapse from the demo, headless only:* *Downside:* the owner never sees item 144's rules in
  the game.

**40. [play] Walkability under destruction: the charge's cut and the wall's height.** A step is
legal only when two tops differ by one voxel at most (`pathing/surface.rs` ~298, ~313). A radius-4
ball leaves a rim step of two or more, a pit units cannot cross; a radius-2 mortar crater stays
walkable; a one-voxel wall is climbable, and walkable on top. **Ruled:** the recommended option,
put to the owner in the interview with decision 10 and joined there to per-material hardness, the
owner's choice against decision 10's recommendation (item 144 (11)). The ruled form is decision
10's: a wall voxel cracks, then breaks; the charge cuts flat down to the wall's footing and removes
every voxel inside its cut whatever its level, stone included, so the gap stays walkable under
hardness; mortar craters stay balls, stepping each voxel inside down a level; bedrock and the bore
are never removed. Since operator item O1 (decision 39), a wall is filled down to the ground, so
the charge's cut, flat at the footing, never leaves wall or terrain above it unsupported, and a
breach never causes a collapse.
- **(Recommended) A demolition charge cuts wall material and terrain inside its radius only down
  to the wall's footing, a flat-bottomed cut level with the ground on either side, so its gap is
  walkable; mortar craters stay balls; a wall box is at least two voxels high
  (`materials.wall_min_height_voxels` = 2), a lower one refused with an E-code; `atk`'s breach
  scenario asserts that the route reopens.** *Downside:* two blast shapes, and a charge cannot dig
  a pit.
- *Balls everywhere:* *Downside:* a breach can strand its own raiders in the pit it made, and a
  one-high wall blocks nothing.

**41. [play] What the autocannon and the mortar fire at, and when a wall is a target.** Wall voxels
are not assets, so `FREE_FIRE` alone never aims at a wall, and the targeting rules of the two
structures are unstated. **Ruled:** the recommended option, put to the owner in the interview (item
144 (11)).
- **(Recommended) The autocannon: the nearest enemy unit in range, else the nearest enemy structure
  or beacon; the mortar: the enemy asset in range with the most enemy assets within its splash
  radius (a clump), ties nearest then lowest id, never a landing point whose splash holds an own
  asset; enemy wall voxels are targets only for a breach: an Attack beacon with `breach_allowed`
  asks the seat's mortars in range to shell the first wall column on its route, and sends its
  charge.** Demo step 4 shells a clump and step 5 the wall. *Downside:* a mortar never shells a wall
  on its own, so a seat with no Attack beacon cannot breach.
- *A mortar may also shell any enemy wall in range under `FREE_FIRE`:* *Downside:* mortars spend
  their fire on terrain while units close in, and walls fall with nobody attacking.

**42. [play] Walls after the fight: repair, and whose walls remain.** Spec §6 lets repair drones
"fix any structure of yours"; a wall voxel is not a structure, and nothing says whose a wall is once
its beacon or its seat is gone. **Ruled:** the recommended option, in the bulk answer (item 144
(11)); under decision 10's hardness it covers wall voxels only, and cracked terrain is nobody's to
repair.
- **(Recommended) A cracked own wall voxel inside a Build beacon's sphere is restored to whole by a
  repair drone at the wall's cost per voxel; a wall voxel counts as its seat's while it lies inside
  one of the seat's live spheres and as neutral otherwise (E14), so when a beacon dies the walls
  only it covered turn neutral, as its ruins do, and on elimination all of the seat's walls do; a
  neutral wall is any seat's to shell or breach and nobody's to repair.** *Downside:* a wall its
  seat built at a sphere's edge and left outside every live sphere (after a recycle, say) is
  neutral; wall repair competes with structures for drone time. *As built* (E14): neutrality is
  the sim's reading and is written into no voxel, so the catalogue holds no neutral wall id, and
  the client, which may not make the reading (AGENTS.md §3 rule 4), draws a wall by the seat that
  built it until S6's art pass, when the gateway would mark neutrality in the view if the owner
  wants it drawn.
- *Walls stay their seat's for good:* *Downside:* a dead seat's walls stay "enemy" to everyone, and
  nobody can repair them.

**43. [play] What repair costs.** Spec §7 puts "repair" second in the Quartermaster's ladder
("defend under attack, then repair, units, build, mine"), so repair spends; the spec states no
price, and the plan's first reading priced none. **Ruled:** the recommended option, put to the owner
in the interview (item 144 (11)).
- **(Recommended) Repair is paid pro rata: restoring hit points costs that share of the asset's
  build cost (HP restored × build cost ÷ maximum HP, an integer, rounded up per repair step), filed
  as a spend request under the Repair band, the drone waiting while it is unfilled; the commander,
  which has no build cost, is repaired free; a wall voxel costs the wall's price (decision 42).**
  Restoring an asset from one hit point to full costs no more than building it. *Downside:* repair
  can starve building in a war, and a seat with no money has drones standing beside damaged
  structures.
- *The arithmetic as built* (the verification review's correction, an engineering call that keeps
  the ruling): rounding up each step on its own would charge up to one `$` a step more than pro
  rata, so a long repair could cost more than the build, against the ruling's own sentence above. So
  the charge is cumulative and needs no state: the cost to full at h hit points is ⌈(max − h) ×
  build cost ÷ max⌉, and a step from h to h′ charges the cost to full before it minus the cost
  to full after it. One repair's charges then telescope to ⌈HP restored × build cost ÷ max⌉,
  so a repair from one hit point to full costs at most the build; `site`'s
  `a_full_repair_costs_no_more_than_the_build` holds it.
- *Free repair (drone time only), the Repair band holding only the drones' fabrication:* *Downside:*
  repairing is always better than rebuilding, so attrition costs only time, and the band the spec
  put second has little to fill.

## 7. Owner decisions during the stage

**All ruled on 2026-10-16 (item 144 (11)),** in the bulk answer, as recommended or, for 33 and 34,
as written; the owner still acts on each when its moment comes (setting the variable, adding the
check, the demo's questions, the fallback's word).

**30. `NIGHTLY_SCENARIOS_ENABLED`.** A repository variable only the owner sets. **(Recommended)**
set when `arms` has merged and its forced run on `tune2`'s numbers is attached. *Downside:*
nightlies run on a stage-old Easy until S5. **Ruled:** the recommended option (bulk); the owner sets
the variable then.

**31. The G3′ alarm job as a required check.** **Ruled:** the recommended option (bulk); the owner
adds the check once it has passed five times. **(Recommended)** the owner adds `g3 budget
(ubuntu-24.04)`, E18's loose regression alarm, to the ruleset's required checks after it has passed
on `main` five times running. *Downside:* a regression that less than doubles the tick shows only in
the owner's certification runs, not in CI, and a shared runner's very slow day can still redden
`main`. *Alternative:* it stays a non-required check. *Downside:* a regression that doubles the tick
merges unless someone reads the job.

**32. Taking a cut: a new authority, ruled with the bulk ruling.** Item 141 delegates merges,
not scope, so taking a cut is not covered by the delegation; it is an authority the owner grants by
ruling this decision. **Ruled:** the recommended option, in the bulk answer (item 144 (11)): the
main session holds this authority, separate from item 141's merge delegation, over section 4.5's
two cuts, Survey's refresh never among them. **(Recommended)** the owner grants the main session
the authority to take section 4.5's next cut at its checkpoint, in the order written, and to record
it in the decisions log the day it is taken; any cut beyond the second, or out of order, is put to
the owner.
*Downside:* a cut lands without a per-cut look. *Alternative:* each cut is put to the owner at its
checkpoint. *Downside:* the lane whose work the cut removes waits on the answer, about a day of path
each time.

**33. At S2's stage demo:** whether the merge delegation runs on into S3, and whether it covers the
radar lab's pull requests (items 140 (3) (a), 141 (1), (2)); the tuning of decision 17's numbers
from `tune2`'s reports; each band acknowledged as Easy's (AGENTS.md §10 item 5); the three-seat
kingmaking figure read (decision 36); and decision 10's blast levels tuned from the duel report.
**Ruled:** stands as written (bulk); its questions are the owner's at the demo.

**34. If G3′ misses:** section 5's fallback, lever by lever, with the owner's word: a lower map
power budget for a per-tick miss, re-certified at the load that budget implies; shorter segments for
a drift miss. **Ruled:** stands as written (bulk); the fallback is taken only on the owner's word.

A lane that finds one of these wrong in the code raises it with the owner rather than building
around it.

---

## 8. Deliberately left out

**Out by AGENTS.md §11:** script runtimes, MCP, an SDK or published API, AI seats, manual control,
flags/branch/repeat, timed weapons and combat boosts (the charge's fuse is a fixed row, not a weapon
a player times), reinforced wall, gate and shield, beacon capture, flowing water, flying.

**Out because a later stage owns it.** **S3:** `report_kinds` and `get_reports` (decision 16); the
own-beacon and enemy-knowledge condition blocks (`playbook.proto` 43–49, 60–69) and E0209;
`list_known_enemies` and `query_area` (`INTERNAL` today); auto-fortify and unit mix (decision 13);
the cut items taken (section 4.5: at most Attack's waves and the crater refill, never Survey's
refresh); FULL and the size budget, P1's counted units (item 139 (5)); the
editor findings of items 139 (2) and 143 (3), (4). **S4:** go-code launches, the Sensor Spire,
licences and their blueprints, the symmetric map, the map re-derivation that re-sizes
`MAX_ROUTE_NODES`. **S5:** Normal and Hard, target spread, Easy's Survey post and chatter (its
Defend guard is S2's, decision 35), three-seat arms (decision 36), the bands re-baselined.
**S6:** art and sound, HP bars beyond the plain ones `view` draws. **S7:** highlights and scrub.
**After S2, beside S3:** the radar lab (items 140, 141 (2)). **Only on evidence:** the commander's
armour, decoys and self-defence weapon, if Hunter's band shows hunting dominates (spec §4). **After
v1:** physics collapse (item 138 (3)), line of sight as a sensor model (item 144 (4)), tunnels in
pathing (E23).

**Out with no owner this stage:** `xtask/src/package.rs`'s `smoke_env` (its next owner); G1's GPU
half on Linux, for want of a Linux GPU machine (section 9, R6).

**Out on principle:** a second hash function, a shadow proto, re-blessing a golden without naming
the rule that moved it, an invented rule where the spec is silent.

---

## 9. Risks, and the measurement that closes each

| # | Risk | What closes it | Where | If it does not close |
|---|---|---|---|---|
| R1 | The sim's serial path (51 days) slips, `fire` above all, which per-material hardness grew | Split seams; checkpoints; section 4.5's two cuts (Survey's refresh never cut, item 144 (11)) | `base`, `ids`, `fire`, `site`, `dfnd`, `atk` | Past the two cuts, the owner's word: decision 1's second option or a further cut |
| R2 | A new hashed field (generations, cooldowns, shells, explored cells, mandate state) is missed | §4.8's three places per PR; save/restore round-trips in each sim PR; lens A | every sim lane | The cross-OS guard and resume tests catch it as a bisect |
| R3 | Repath bursts under craters break the p99 | The measured burst frequency; the cap re-derived | `g3h`, `g3r` | Section 5's fallback: a lower power budget |
| R4 | The support search's worst case | `max_region_voxels` and `max_visits_per_tick` measured inside G3′, at an adversarial tick | `fall`, `g3h` | Lower bound rows, with their in-world reason (`g3r`) |
| R5 | The broadphase grows super-linearly under combat queries | Its share measured; cell size from range (E11) | `g3h` | Item 67's rejected hierarchical grid, reconsidered on evidence |
| R6 | Re-meshing under a crater stream misses G1's three frames; G1's Linux GPU half never runs | `view`'s stream through the CPU proxy on three runners; an incremental relight | `view` | Coarser destruction (G1's fallback), the owner's |
| R7 | The nightlies measure Easy's weakness, not balance | Bands read against Easy (item 144 (6)) | `arms`, `demo2` | Re-baselined at S5 |
| R8 | Eighteen contract PRs, beside the main session's docs PRs, queue on review | One proto and one `xtask` PR at a time; the week of float | section 4 | Decision 1's extension grows |
| R9 | The G3′ job is flaky on a shared runner | It is a loose alarm at the gate's numbers doubled, never the gate; the gate is certified on the owner's machine under Windows and WSL2 Linux (E18); five passes before it is required (decision 31) | `g3r`, `demo2` | It stays a non-required check |
| R10 | E1 changes what an id is, and every chain with it | An additive layout that keeps every old id's value; one PR, early, re-blessed once with reasons; a test per reset column | `ids` | — |
| R11 | Breach on a 2.5D graph | Breach as a cost mode over wall columns (E23) | `atk` | Breach by mortar only; the charge waits |
| R12 | Friendly-fire avoidance leaves a mortar unable to fire near its own assets | The duel report's mortar lines | `tune2` | The owner re-rules avoidance at the demo |
| R13 | A collapse carrying an asset is unreachable in ordinary play (1-Lipschitz terrain solid from bedrock, walls filled to the ground, Mine's structure rule; only a Mine pillar's top, which carries nothing, can fall), so the row's feature is unseen in play | Said plainly; the demo's beat staged headless and said to be staged; `fall`'s `collapse_credits_the_blast` pins it through test seams (decision 39, operator item O1) | `fall`, `site`, `demo2` | A span limit, on the owner's word, or physics after v1 |
| R14 | E7's whole-sim sweep (about 450 sites) grows `base` past its estimate | Its own commit set; the classification in the PR; the guard test | `base` | The sweep's last files carried by `ids` as a named commit, said in both PRs. **Taken** (decisions-log item 145 (5)): `base` counted about 552 sites and carried 216, in six files, to `ids` |
| R15 | The bulk ruling comes late and holds `con3`, then `ids` | **Closed:** it fell on 2026-10-16, before wave 1 (item 144 (11)) | section 4.1 | — |
| R17 | Per-material hardness (decision 10) makes craters in stone too shallow, or mortars too weak against dug-in ground | The duel report's dirt and stone lines at the proposed `blast_levels` | `tune2` | The owner re-tunes `blast_levels` at the demo, within the catalogue's 1 to 3 |
| R16 | The certified figure differs between Windows and WSL2 Linux by more than the codegen's 10–20 % | Both measured, the slower deciding (spike G3′ step 7) | `g3r`, `demo2` | The budget is set by the slower, and the gap is recorded |

---

*Sources: `docs/spec/pharmakos-spec-v0.6.html` §3–§9, §11, §14–§17, §19; decisions-log §2.7 items
13, 17, 40, 63–69, 90, 91, 104, 116, 123, 127, 128, 133–144; `docs/design/s1-plan.md`;
`docs/design/skeleton-plan-w6-notes.md`; `docs/placeholders.md`'s S2 section;
`docs/spikes/G3prime-segment-budget.md` §9; `docs/spikes/G1-destruction-remesh.md`; AGENTS.md §3–§6,
§9–§12; `.claude/workflows/wave-lanes.js`; `scripts/merge-train.sh`; the code at `7a351e7`.*
