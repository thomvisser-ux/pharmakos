<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# S2, destruction and combat — plan for spec section 17, the wk-29.5 row

**Adopted 2026-10-16 under decisions-log §2.7 item 144 (8).** The main session merges this plan
after two adversarial reviews, under the merge delegation item 141 renewed for S2, and S2's first
wave starts at once. The owner was interviewed on the questions that shape play before the plan was
written (item 144 (1) to (7)); every other recommendation of sections 6 and 7 is put to the owner in
bulk and stands as its **(Recommended)** option, open to the owner's overrule (item 144 (8), as item
127 (1) did for S1). The **[play]** marks name the decisions the bulk question names one by one.

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
   approach, an autocannon and a mortar. The editor shows the wall's price, `$` 1 a voxel, in the
   projection, and W0501 is gone from `seam_choice: SAFEST`.
2. **Round 1's Push.** With the Build beacon's `terraform` set to `BOTH`, a build drone levels the
   mortar's footprint before it builds: the ground under the target rises or falls to the anchor's
   height, a voxel at a time, and each voxel leaves the treasury at the wall's price. The wall goes
   up voxel by voxel. The core's own autocannon stands from the first tick.
3. **Round 2.** Easy's raiders come for the nearest structure. The owner's autocannon and the core's
   cut them down; each dead raider leaves a wreck. A raider reaches the mortar and damages it. The
   Build beacon's repair drone walks to the mortar and repairs it, then salvages the wrecks, and the
   feed shows `$` coming in at 25 % of each wreck's build cost.
4. **Round 3.** The owner's mortar shells Easy's wall. The shell is slow and visible in flight. It
   lands as a crater; wall voxels crack on the first blast and break on the next, and terrain goes
   at once. A unit standing where the shell lands takes splash damage, the owner's own included,
   since friendly fire is on; the mortar never chooses a landing point whose splash holds an own
   asset.
5. **Round 4.** The owner's Attack beacon, `breach_allowed`, gathers raiders at its staging point
   and launches on force size. The wall blocks the route, so a raider carries a demolition charge to
   the wall's footing and plants it; the fuse runs out and a ball of voxels goes. The wall section
   above the gap has nothing under it, so it **collapses**: it falls, everything standing on it
   falls with it and takes the support rule's flat damage, and the kill credit goes to the owner,
   whose charge detached it (item 144 (1), (2)).
6. **Round 5.** The owner's Attack beacon targets Easy's commander from its last sighting and
   retargets live while a raider sees it. Easy, whose commander died in round 4's breach, has added
   a Defend guard on it (spec §14): its guards escort the commander inside that beacon's sphere and
   intercept. The owner's own commander is hunted in turn by Easy's raid: it takes damage, the 20 %
   reflex walks it to the safest own beacon, where the Build beacon's repair drone heals it, and
   when it dies it respawns at the core after the growing delay. A commander is not an asset, so its
   death credits nobody (spec §4).
7. **A beacon falls.** The owner's mortars destroy one of Easy's beacons. Its mandate ends at once,
   its structures turn to neutral ruins, its units retreat and re-home to Easy's nearest beacon, and
   the owner's repair drones salvage the ruins inside their own sphere.
8. **The recap** shows each seat's kills and losses, a "shared kill" where two seats' damage
   finished an asset, and the value each seat destroyed.
9. **The end.** The match ends on the first tick Easy has no beacon left (`last_seat_standing`), or
   at round 6's limit, where the final audit names the winner and its destroyed-value term is now
   not zero (X-08).

Beside the demo: the G3′-real certifying report (section 5); the nightly report, three arms against
Easy on the fixed seed set, each band written and acknowledged as Easy's (item 144 (6)); and the
headless balance and duel reports from which the owner tunes combat's numbers (item 144 (8)).

### 1.2 The machine-checkable half — AGENTS.md §10, item by item

| §10 item | What closes it at S2 | Task |
|---|---|---|
| 1. `cargo xtask ci` green on the matrix, the OSes agreeing on the chains | Every lane green on the three-OS matrix; the cross-OS guard over every chain, digest and path hash, the new S2 scenarios' chains included | all; `demo2` |
| 2. Hashes committed, every movement explained | The re-bless ledger of section 4.2; each chain-moving PR names the rule and the first tick that moved | `east`, `base`, `fire`, `site`, `fall`, `dfnd`, `surv`, `atk`, `opra`, `tune2` |
| 3. Golden playbooks round-trip, verify and render identically | The two new templates under `library/`, the three nightly arms, and the new arms in `tests/golden/plan-core/` and `tests/golden/verifier/` | `vfy2`, `oprd`, `opra`, `arms` |
| 4. Headless scenarios assert on events **and** hashes | Nine new scenarios under `scenarios/s2/` (section 3 names each lane's), beside the nine committed today | `fire`, `site`, `fall`, `dfnd`, `surv`, `atk` |
| 5. The nightly adversarial scenarios | Rusher, Turtle and Hunter against Easy on the fixed seed set, nightly, each with its written alarm band, every band acknowledged as Easy's at S2's close and re-baselined at S5 (item 144 (6)); `NIGHTLY_SCENARIOS_ENABLED` set by the owner (decision 30) | `night`, `arms`, `demo2` |
| 6. Depth task and gate, or the written fallback | G3′-real certified by a recorded run (section 5), or the spec's fallback taken in writing | `g3h`, `g3r`, `demo2` |
| 7. Docs in the same PR | The spec amendments and AGENTS.md sentences of section 4.4, the main session's docs PRs | main session |
| 8. The owner reviews the demo | §1.1, from `docs/demo/s2-run-sheet.md` | `demo2` |

### 1.3 What "ends playable" means here

Every item of the row, of §2.7's additions above, and of the register's S2 rows and carries (section
2), and nothing else; less whatever the written cut order of section 4.5 has moved to S3 by the time
the stage closes. Where the spec is silent the plan's readings are owner decisions (sections 6 and
7), none built before it is ruled.

---

## 2. What §2.7 settled for S2, and where each ruling lands

| Ruling | What it says | Task |
|---|---|---|
| 144 (1) | Everything on a detached region falls with it and takes one flat damage; the core's bore is never detachable | `fall` |
| 144 (2) | Collapse's kill credit goes to the seat whose edit detached the region; an own edit credits nobody | `fall` (the seat on the edit: `fire`) |
| 144 (3) | Mine never detaches a region carrying any seat's structure | `fall` (a named line in `mining.rs`) |
| 144 (4) | Unit vision is a radius, no line of sight; line of sight a seam of the one sensor interface | `base` (the seam), `con3` (the rows) |
| 144 (5) | Terraform: FILL raises a footprint to its anchor's height, DIG lowers it, BOTH both; FILL and BOTH refill craters and trenches in the sphere to the generated surface; each voxel at the wall's cost | `site` |
| 144 (6) | Nightlies play Easy, about twenty seeds, the 65 % band, acknowledged as Easy's at S2's close, re-baselined at S5 | `night`, `arms`, `demo2` |
| 144 (7) | The whole row planned, with a cut order written in advance | section 4.5 |
| 144 (8) | S2 runs as S1 did; G3′-real is the depth task with the spec's fallback; owner rows in bulk; numbers in range, checked headless, tuned at the demo; the plan merged after two reviews, wave 1 at once | section 5; `tune2`; section 4 |
| 138 (3) | A bounded, incremental, totally ordered support rule; its place in the phase order, its cache invalidation, its allocations, its worst case measured in G3′; its constants as rows | `fall`, `con3`, `g3r`; design in section 3's `fall` |
| 138 (4) (c) | Spheres, unit vision, pickets and Survey under one sensor interface with two outputs | `base`; `dfnd` (pickets), `surv` (Survey) |
| 128 (d) | Repair threshold, rebuild destroyed and terraform take effect in S2; `BuildSettings` 6 and 7 held until S2 | `site`; decision 15 for 6 and 7 |
| 123 | The core's autocannon arrives with combat | `fire` |
| 104 | The gateway doors for Defend settings, Attack settings and `beacon_under_attack` close at S2 | `dfnd`, `atk` (named places in `crates/gateway/tests/methods.rs`) |
| 63, 65, 69, 91 | G3′'s world totals; the 190 kW budget, `kw_per_unit`, `reserve_percent` and `map_ceiling_kw` moving together; the repath cap re-derived once bursts are measured | `g3h`, `g3r`, `tune2` |
| 64, 68 | Do not charge G2's CSR rebuild twice; re-measure process memory growth | `g3h` |
| 139 (1) ruling 3 | `expand_east`'s example moves: a timeout the walk can meet or a nearer first site | `east` |
| 139 (1) ruling 7 | The fielding limits (S1-37) are revisited at G3′-real | `base` (live counts), `tune2` (totals) |
| 139 (1) ruling 10 | The Build list's settings row is atomic, failing with a new additive `StepFailure` id | `base` |
| 139 (6) | The ruled markers re-dated by a comment-only sweep granted here; `scenarios/` comments; `economy.rs`'s BMI expansion; `rules.proto`'s stale comments with S1-42 and ruling 14's markers; Expand & Mine's wording; S1-01's sweep and `gamectl --operator` at S2's start | `redate`, `east`, `base`, `con3`, `oprd`, `msrv`, `night` |
| 143 (5) | `-- --rounds <n>`, test-only, in a client lane; the line's format unchanged | `rnds` |
| 141 | The merge delegation runs to S2's stage demo; the radar lab starts after S2, beside S3 | section 4.3; decision 33 |
| 135 (2) (d), (f), (g), (j); 136 (5) | `seal_commitment`'s E-code (S2-14); `committed_dollars`' comment and fields 5 and 6 `optional`; the event vocabulary additive; `build`'s "Found, not fixed" to S2's first sim and verifier lanes | `vfy1`, `con3`, every lane adding an event, `base` |

### 2.1 The register's rows, each placed

| Row | What | Where it lands |
|---|---|---|
| S1-01 | The MSRV half: `rust-version` and clippy's `msrv` from 1.85 to 1.98 | `msrv`, S2's first PR |
| S1-13 | `BeaconSummary.core`'s stale marker | `con3` |
| S1-34 | Unit sight radius as a row, and every copy | `con3` (rows), `base` (the sim), `wgw` (the gateway's `sight_key`, `Sight`, `SphereVision`) |
| S1-37 | The fielding totals, revisited at G3′-real | `base` (live counts), `tune2` (the totals, from `g3r`) |
| S1-42 | `fingerprint.rs`'s doc, `proto.rs`'s two test messages, `rules.proto`'s constant-swap comments | `con3` |
| S1-43 | Uncommented workspace versions checked against the registry | every lane (standing rule) |
| S1-51 | `crates/verifier/src/interface.rs`'s stale docs | `vfy1` |
| S2-01, M-01 | The nightly flags, a seed set, a machine-readable result, `win_rate_percent` an integer | `night` |
| S2-02 | The safe playbook's rescue rule | decision 6; `oprd` and `wgw` |
| S2-03, S2-04 | `AVOID_KNOWN_THREATS`; `safest`'s threat term | decision 12; `dfnd` (the model), `atk` (the route cost) |
| S2-05 | `HEADROOM_VOXELS` as a row | `con3` (row), `base` (reader) |
| S2-06 | `MAX_ROUTE_NODES` | decision 24; `base` |
| S2-07 | The edit and damage queues | engineering call E5; `base` |
| S2-08 | The structure and wreck table totals | decision 25; `base`, `tune2` |
| S2-09 | The broadphase query radius | engineering call E11; `fire` |
| S2-10 | A dropped Build target strands what was paid for | decision 26; `site` |
| S2-11 | What a seat sees of an enemy asset | decision 5; `con4`, `wgw` |
| S2-12 | `get_view`'s delta mode | decision 27; measured in `view` |
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
| Whether to narrow the estimate's `NOT_FOUND` drawing, with `editor.rs`'s doc | item 136 (5) | `view` |
| `crates/client-gdext/src/rules.rs`' test comment (~283), "the owner's to tune at S1's demo", ruled by item 139 (1) ruling 6 | item 139 (6) | `view` |
| The root `Cargo.toml`'s research NOTE, which names four of the six guarded crates | handoff NEXT 3 | `msrv` |
| `gamectl`'s `b_01`-as-core texts (`scenario/run.rs`, `seat.rs`, `strings.rs`) | handoff NEXT 3 | `night` |
| `crates/bench/tests/harness.rs`'s doc list of restated items | item 133 (5) | `g3h` |
| G4's bar, 10 matches of 9 600 ticks, which "stays with the nightly workflow and S2" (`xtask/src/main.rs` ~426) | item 116 | `night` |
| `ci.yml`'s perf-alarms comment (~986), which names the tick-minus-pathing mean as S2's | item 116 (6) (b) | `g3r` |
| `crates/sim/tests/allocations.rs` without walkers; a moving crowd's allocations | item 137 (2) (j) | `atk` |
| `voxels.rs`'s "Nothing fills the queue yet" (~274), stale since `mine` | this plan's code check | `base` |
| Expand & Mine's "ore seam", where the spec (§9) and the client say "scrap seam" | item 139 (6) | `oprd` |
| `xtask/src/package.rs`'s Godot import isolated (`smoke_env`) | handoff NEXT 3 | **left out**: no S2 lane owns `package.rs`; it stays with that file's next owner |
| The register's line (~411) giving `upload.rs`'s K and B to S2's exit, where `crates/mesher/src/upload.rs` (~44–49) says S6's art pass | digest | the main session's register edit with this plan's docs PR: the code is right |
| `crates/verifier/src/limits.rs` (~203) dating the reach window S2/S3, where `rules.proto` (~727) says S3 | digest | `vfy1` rewords `limits.rs` to S3 |

### 2.3 Engineering calls, decided here (the owner can veto each)

These are hygiene, types, layout and test gates under the operator's global rules and AGENTS.md §4,
not rules of play. Each is the main session's call.

- **E1. The fielding totals count live assets, and rows are reused under generational ids.** Today
  `fabricate` counts every row a seat ever produced (`world.rs` `fabricated_by`, "dead or alive (a
  row is never freed)"), so under attrition item 63's world totals become a lifetime cap — 100 units
  a seat at three seats, gone after the first 100 deaths — which the no-artificial-caps rule forbids
  and which spec §6 contradicts ("a mandate's fabricator produces to the work in hand, not to a
  count, so there are no counts or caps"). `base` changes the count to live assets and reuses dead
  rows. The id scheme: an id is the row and that row's **generation**, `(row: u16, generation: u16)`
  packed into the existing `u32` of `UnitId`, `StructureId`, `BeaconId` and `WreckId` (`tables.rs`);
  a dead row is free once its death has settled (its wreck made, its kill credit booked); the lowest
  free row is reused first, and its generation goes up by one, so a stale reference (a kill-credit
  slot, a sighting, a pinned Attack target, an event value) never names the new asset. The same
  scheme covers structures, beacons and wrecks. Because a beacon row no longer stays at zero HP for
  the whole match, X-08's "beacons lost" becomes a per-seat hashed counter; the per-seat `b_NN` and
  per-viewer `e_NN` names are unchanged, since they count placements and sightings, not rows. A row
  whose generation would pass `u16::MAX` is never reused again (checked arithmetic, never a wrap),
  counted under its reason; no match comes near 65 535 reuses of one row. This is determinism code:
  the hash, the snapshot (`SNAPSHOT_VERSION` 9 → 10) and every golden move together in `base`.
- **E2. One damage latency rule: every damage lands in the tick it is dealt.** The code's comment
  intends "anything filed while [Combat] ran waits for the next tick" (`world.rs` `phase_combat`),
  but no order filed in the sim survives a tick today, and the damage queue is in neither the hash
  nor the snapshot, so an order held across a tick boundary would be a latent desync. S2 makes it
  uniform the other way. Combat becomes: (1) every armed asset chooses its target from the
  post-Movement state, in total order, and fires; (2) shells whose flight ends this tick land; (3)
  every order filed in (1) and (2) is applied, in filing order, kill credit booked first, as today.
  Fire is therefore simultaneous: no seat's shot lands before another's choice. Damage from the
  Voxels phase (collapse's fall damage) is applied at the end of that phase by the same apply
  function. Nothing the sim files is pending at a tick's end, so the queue is never state. The
  host's `file_damage` seam (`crates/gateway/src/host.rs` ~607, a test seam no handler reaches)
  files between ticks and lands at the next Combat; a test that files and then snapshots before
  stepping is refused with a typed error rather than saving a pending order. A death in Voxels
  settles in the next tick's KillCredit phase, over the dead asset's hashed HP and credit slots.
- **E3. Damage and repair are types that cannot hold a wrong sign.** `Hp::damaged_by` heals on a
  negative amount with no cap (`math/quantity.rs` ~59). `base` makes damage a `Damage` newtype
  holding a positive amount (`NonZeroU32`-backed) and repair a separate `Hp::repaired_by(Repair,
  max)` that stops at the asset's maximum by stated rule. A zero-damage order cannot be built.
- **E4. Every dropped order is counted under an enumerated reason.** `phase_combat` skips an order
  whose target row does not exist (`world.rs` ~2662), and an order on an already dead target applies
  nothing and books nothing, silently. `base` gives each drop a reason code
  (`DamageDrop::NoSuchRow`, `::StaleGeneration`, `::AlreadyDead`), counted in an unhashed counter
  beside `seams::WorkCounter` and asserted by tests; two shots that kill one target in one tick are
  the normal case for `AlreadyDead`, and the counter makes it visible rather than an error.
- **E5. Queues are sized so they cannot fill, and a full queue is a typed error.** The edit queue
  (64) and the damage queue (256) are guesses (S2-07). `base` sizes both at load from the world's
  table room — each live asset files at most one edit and at most one direct order per tick, and a
  landing shell's splash at most one order per asset, all bounded by the table rooms — so neither
  can fill in a legal world; `request_damage` and `request_voxel_edit` return a typed `QueueFull`
  error, propagated, and a test holds that the sizing covers the worst case. Mining's existing
  back-off on a full queue becomes a match on that error.
- **E6. The broadphase cannot overflow, by construction.** `phase_broadphase` checks the rebuild
  with a `debug_assert!` only (`world.rs` ~2578), so a release build would query an empty grid.
  `base` sizes the grid from the unit table's room at construction and returns a typed error there
  if it does not fit; the per-tick rebuild is then infallible and its `bool` goes.
- **E7. The remaining silent fallbacks in the sim become typed errors.** `mapgen.rs`'s
  `i32::try_from(..).unwrap_or(i32::MAX)` for the core's and the commander's HP (~808, ~1008) become
  load-time refusals of an out-of-range row; `mapgen.rs` (~799) and `world.rs` (~582)
  `u8::try_from(..).unwrap_or(u8::MAX)` seat conversions and `survey.rs`'s
  `usize::try_from(..).unwrap_or(0)` (~56) become checked reads; `runner.rs`'s `MatchState::new`,
  which clamps `round_limit` to one and every ladder entry to one tick ("clamped, not trusted",
  ~281), takes validated `MatchSettings` built through a constructor that refuses zero with a typed
  error (the gateway already refuses a zero round limit, `host.rs` ~288).
- **E8. The verifier refuses, by its own code, every construct the sim refuses.** Today a playbook
  can pass the verifier and be refused by `Plan::compile` at `submit_plan` as `INVALID_ARGUMENT`
  (`crates/gateway/src/surface/planning.rs` ~46–62), so the editor's verdict and the sim disagree.
  `vfy1` gives every construct the sim's `PlanError::NotAtThisStage` still refuses an E-code of its
  own (one per construct, in a new E00xx run after E0008), and a test in `crates/verifier/tests`
  walks the sim's refusal table, which the verifier may read through its `default-features = false`
  dependency, and fails while any refused construct has no E-code. Each sim lane that lifts a
  refusal lifts the verifier's E-code in the same PR, through a named place in the verifier's
  refusal table and its case (section 3), so the two never disagree on `main`.
- **E9. `get_beacon`'s prose follows the structured summary.** The prose prints a visible enemy
  beacon's mandate and HP (`crates/gateway/src/surface/knowledge.rs` ~442–462, `strings.rs` ~507)
  while `BeaconSummary` withholds both (~1102–1112). `wgw` makes the prose say only what the summary
  says; when decision 5 rules what a seat sees, both follow the ruling together.
- **E10. One sensor interface.** `base` adds `sight::Sensor`: every source (a beacon's sphere, a
  unit's or structure's vision radius from its rules row, a picket, a Survey scout) is a sensor, and
  the interface has two outputs, the live view the fog filter reads and the knowledge store's
  sightings and reach (item 138 (4) (c)). Its line-of-sight method is a seam that answers "visible"
  for every point in the radius (item 144 (4)); a later model replaces the method, not its callers.
  Vision queries go through the broadphase grid rather than `survey.rs`'s quadratic scan. A
  sighting's age stays derived from the tick it was taken at.
- **E11. The broadphase query radius is a weapon's range.** `BROADPHASE_QUERY_RADIUS_VOXELS` (16) is
  referenced nowhere (S2-09). `fire` deletes it. A query of radius r touches (2⌈r / cell⌉ + 1)²
  cells, so any range is answered exactly whatever the cell edge; the edge stays the tuning row
  `broadphase.cell_size_voxels`, which item 67 tied to the largest query radius and unit density and
  which `g3r` re-derives from the measured ranges and density.
- **E12. Fire's total order.** Armed assets choose and fire in (seat, kind, id) order; a target is
  chosen from the broadphase over post-Movement positions, by the asset's targeting rule (section 6,
  decisions 8, 12 and 21), ties by squared distance and then lowest id. Spread, where a weapon has
  any, is drawn from `Stream::Combat` keyed by (tick, seat, asset), the stream's first use; no other
  purpose draws from it (AGENTS.md §4.7).
- **E13. Shells are hashed entities.** A mortar shell in flight is a row in a new shell table
  (target point fixed at fire, landing tick, seat, weapon), in the hash, the snapshot and the
  goldens in `fire`'s PR, under a table room derived at load from the structure room and the
  mortar's rows, so the table cannot fill. A shell lands where it was aimed, so a unit that moves is
  missed: the spec's "slow arcing shell that can be dodged" is the shell's flight time, not a hit
  roll.
- **E14. Walls know their owner through their material.** "Your own voxels" (spec §9, friendly-fire
  avoidance) needs a wall voxel's owner. `fire` gives walls material ids from 9 up, one per seat and
  damage level (decision 10), so ownership and damage live in the voxels the chunk digests already
  hash and no new table is needed. Ruins and built structures stay entities with footprints.
- **E15. The destroyed-value term is accumulated per seat.** `audit::destroyed_value` is always zero
  and `credit.rs` says the split is "emitted and not accumulated" (~33). `fire` adds a per-seat
  destroyed-value column, credited at each kill by the seat's settled share of the asset's build
  cost (largest remainder, spec §3), hashed and snapshotted; the audit reads it (decision 7).
- **E16. Wrecks are made on death.** A dead unit or structure leaves a wreck worth
  `economy.salvage_percent` of its build cost; nothing produces a wreck today (`world.rs` ~226).
  `fire` makes them; `site`'s drones salvage them. A ruin (a structure on a destroyed beacon) stays
  a structure row turned neutral, as now.
- **E17. The event vocabulary grows additively.** Each lane that needs a kind adds it at the end of
  `EventKind` (next free id 38) and each failure at the end of `StepFailure` (next free id 13, 10
  retired), with a new name, never renaming or reusing one (AGENTS.md §5, item 135 (2) (j)). Events
  are derived output and never hashed. A lane that adds a kind takes, as a named place, its arms in
  `crates/gateway/src/strings.rs` (`event_text`, `audience_of`) and their tests, edited only when no
  running lane owns `crates/gateway`, or as its last commit after that lane merges (section 3).
- **E18. The G3′ CI budget check is its own command and its own job, not a `ci` step.** A `ci` step
  must be green on any machine that runs it ("green locally means green in CI", AGENTS.md §9), and a
  wall-clock budget is a property of the machine. So `g3r` adds `cargo xtask g3-budget` outside the
  step table, like `package`, and a CI job `g3 budget (ubuntu-24.04)` that runs it at the gate load
  and fails when the tick p99 exceeds 25 ms or the mean exceeds 12.5 ms (spec §16's numbers, no
  margin invented); the Windows and macOS legs publish the same figures as notices; the certifying
  figure is the recorded run on the owner's machine (section 5). It follows the docs-only fast path.
  It is a contract change (`xtask`, `.github/workflows/ci.yml`); whether the job joins the ruleset's
  required checks is the owner's (decision 31).
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
- **E22. `expand_east` gets a nearer first site.** Ruling 3 left the choice open. The timeout cannot
  be what moves: `tune`'s report found the walk from either seat's core is 290 s or more at the
  commander's speed (`rules/README.md`, `commander_cost_per_second`), longer than round 1's 180 s
  Push, so any timeout the walk meets draws W0701. `east` moves the first site to the nearest legal
  site east of seat 0's core that `estimate_route` reaches inside 100 s on the `expand-east-segment`
  seed, keeping the 120 s timeout and the example's shape; the spec's worked example moves with it
  in the main session's docs PR.
- **E23. Pathing under destruction stays 2.5D.** The search graph has one node per column
  (`pathing.rs`), so a crater lowers a column and a cave under an unchanged top is invisible to it.
  Breach is a second cost mode over that graph (section 3, `atk`): a column whose top is wall
  material is passable at a demolition cost for the breach query only. Tunnels are not modelled,
  which Mine's "never undermines" (item 144 (3)) and the absence of tunnelling in v1 make safe.
- **E24. The S2 `get_view` entity fields and any new message shapes are the smallest that serve
  their consumers,** as `con2`'s were (item 130), and the two reviews read them as a contract
  decision.
- **E25. An unset `roe` is not an engineering call.** `playbook.proto`'s header reserves any
  addition to its list of omitted settings that read as defaults to an owner decision, so the
  default `roe`, and which chains it moves, is decision 4.

---

## 3. The tasks

Conventions for every task, as in S1: its own worktree (`git worktree add ../pharmakos-<id> -b
<branch> main`), target directory `D:/build/<id>`, **one crate one agent** (AGENTS.md §6), rebase on
`main` before the PR, conventional commits, `git commit -s` with the co-author line above it, SPDX
headers, `cargo xtask ci` green before the PR opens, and every guessed value a `// PLACEHOLDER:
<what> — <who>, <when>` listed in the PR body. A lane opens its PR and stops; the main session
merges (item 141). "Contract PR: yes" means an AGENTS.md §5 path; the PR body names the paths.
Estimates are agent-days (ad) including iteration to green, not review latency.

**Standing rules carried from S1.** `xtask` and `.github/workflows/**` belong to no lane except the
one named for them, and at most one such PR is open at a time (`msrv` touches neither; then `night`,
`g3h`, `g3r`, `tune2`, in that order). At most one `proto/**` PR is open at a time (`con3`, then
`con4`, then `tune2` only if a row moves or retires). Each `→` in section 4.1 is a separate
`wave-lanes.js` run. Each task's Implements line is its lane's `items`. Every task over 8 ad carries
a split seam written before it starts. A lane that adds a committed scenario owns its path constant
and its `SCENARIOS` entry in `crates/gamectl/tests/scenarios.rs` and its line in
`the_committed_list_is_the_whole_set` (items 129 and 135 (2) (l)). A lane that changes `library/`,
`rules/`, `packaging/`, `LICENSES/`, `REUSE.toml` or `godot/` is packaged locally by the main
session before it merges (S1's decision 9). A lane that changes `godot/scripts/` runs the headless
watch check by hand with the `client extension` job's command (item 143 (2)). Every lane re-dates,
in the files it owns, any marker the `redate` sweep had to leave out.

**Named places for the sim's refusals and events.** A sim lane that lifts a `NotAtThisStage` refusal
is granted, in the same PR, the verifier's refusal table and its case for that construct (E8), and
the gateway's matching row in `crates/gateway/tests/methods.rs`' `mandate_and_rule_gaps()` (~2457),
where the S2 rows are (not `two_door_cases()`). A sim lane that adds an `EventKind` or a
`StepFailure` is granted their arms in `crates/gateway/src/strings.rs` and the tests that pin them
(E17). Either grant holds only while no running lane owns that crate; when one does, the sim lane
makes that edit as its last commit after the owner merges, as S1's `build` did with the gateway's
fingerprint caller.

---

### `msrv` — the whole-tree MSRV sweep (S1-01), S2's first pull request

- **Owns:** the whole tree for one commit pair: `Cargo.toml` (`[workspace.package] rust-version` and
  the research NOTE), `clippy.toml` (`msrv`), and every file `cargo xtask ci --fix` rewrites.
  **Branch** `build/msrv-1.98`, worktree `../pharmakos-msrv`, target `D:/build/msrv`. It runs alone:
  no other lane holds a crate while it is open (item 129 (2) (b)).
- **Builds:** `rust-version` and `msrv` 1.85 → 1.98, their markers reworded as discharged; then
  `cargo xtask ci --fix` across the workspace for the lints the raised MSRV turns on, in a second
  commit so the mechanical diff reads apart from the setting; any lint the fix pass cannot apply is
  fixed by hand or, where it would defeat a determinism lint, stopped and raised. The root
  `Cargo.toml`'s research NOTE (~103–106) names all six guarded crates (`plan-core`, `verifier`,
  `operator`, `gateway`, `gamectl`, `bench`; `xtask/src/main.rs` `GUARDED_PACKAGES`).
- **Implements:** register S1-01; item 129 (2) (b); handoff NEXT 3's research NOTE.
- **Needs:** this plan merged. **Acceptance:** `cargo xtask ci` green; **no chain and no golden
  moves** (a lint fix that moves one is a behaviour change, and the lane stops and says so).
- **Contract PR:** yes (`clippy.toml`, the workspace manifest). **PLACEHOLDERs:** none.
  **Agent-days:** 0.5–1.

### `redate` — the comment-only re-dating sweep (item 139 (6))

- **The grant, under AGENTS.md §6 and item 141 (3).** This lane may edit **comments and nothing
  else** in these files, re-dating every `PLACEHOLDER` whose "when" names S1's demo or S1's stage
  demo to the ruling that settled it (item 139 (1) rulings 4 to 9) or to the stage that now owns it:
  `crates/operator/src/easy.rs` (1, ~121), `crates/bench/src/decision.rs` (1, ~83), and
  `rules/README.md`'s Settled? table (16 `**PLACEHOLDER**` cells), which is documentation rather
  than data and enters no hash. Each marker keeps its what, who and when grammar, and none is added
  or removed. The PR shows `cargo xtask placeholders` before and after, lists every file it touched,
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
  item 141 (3). **Needs:** `msrv` merged. **Acceptance:** `cargo xtask placeholders --check` green;
  the register's before-and-after in the PR body; no golden moves. **Contract PR:** no (no contract
  path is in the grant). **Agent-days:** 0.5–1.

### `east` — ruling 3: `expand_east`'s nearer first site, and the `scenarios/` comments

- **Owns:** `examples/playbooks/expand_east.jsonc`,
  `scenarios/skeleton/expand-east-segment.scenario.jsonc`, `scenarios/README.md`,
  `scenarios/s1/unaffordable-deploy.scenario.jsonc`'s comment, and every pin of the example's
  content as named places, granted because no other lane holds these crates in wave 0 (AGENTS.md
  §6): `tests/golden/proto/expected.expand_east.json`, `tests/golden/plan-core/expand_east/`,
  `tests/golden/verifier/expand_east/` with `crates/verifier/tests/cases/expand_east.json`,
  `tests/golden/scenarios/expand-east-segment/` (chain and event log), and the tests that read the
  site: `crates/gamectl/tests/balance.rs` (`the_expand_east_column_reads_the_first_step` and its two
  markers), `crates/gamectl/tests/verify.rs`, `crates/client-gdext/tests/common/mod.rs` and
  `src/editor.rs`'s test fixture, `crates/gateway/tests/methods.rs`' template case and
  `crates/proto/tests/proto.rs` (~254–306) wherever they assert the old voxel. **Branch**
  `fix/scenarios-expand-east`, worktree `../pharmakos-east`, target `D:/build/east`.
- **Builds:** E22's move: the first site becomes the nearest legal site east of seat 0's core that
  `estimate_route` reaches inside 100 s on the scenario's seed, with the 120 s timeout kept; the
  example and its canonical copy stay byte-identical to each other;
  `deploy-and-visit.playbook.jsonc` is checked, and moved only if it copies the example.
  `unaffordable-deploy`'s stale header (~18–19, "the `$` projection … not yet built", which `proj`
  built) and `scenarios/README.md`'s walker note (~155–162, which still names S1's demo; the cap is
  S2-16's) are reworded.
- **Implements:** item 139 (1) ruling 3, (6). **Needs:** `msrv` merged. **Acceptance:** the
  `expand-east-segment` chain and event log re-blessed, the first diverging tick named and the step
  that now succeeds named; the `expand_east` verifier, plan-core and proto goldens re-blessed with
  the reason; `balance.rs`'s column reads the new site and its report says the walk fits. **Contract
  PR:** no (golden content, not format); the main session's docs PR moves the spec's worked example
  (section 4.4). **Agent-days:** 1–1.5.

### `base` — `crates/sim`: the carries, the fallbacks, live fielding and the sensor seam

- **Owns:** `crates/sim`, every golden its changes move, the named places of section 3's preamble.
  **Branch** `feat/sim-s2-base`, worktree `../pharmakos-base`, target `D:/build/base`.
- **Builds:**
  - **Ruling 10.** `set_mandate_settings`' Build list is atomic: a row that cannot write the whole
    list (the seat's target table full part-way through) writes nothing and fails its step with the
    new `StepFailure` id 13, `targets_unwritten` (E17); `replace_build_targets`' carry of a paid
    structure covers every entry, not the first eight (`TARGETS_PER_BEACON_SLOTS`, ~250);
    `add_bound`'s and `add_area`'s `false` become typed results the row propagates (item 136 (5)).
  - **Load-time refusals** of the rows `structure_cost` and `structure_max_hp` answer zero for
    (~1210–1221) and of `mandate.rs`' `backlogged` threshold, so all three readers become
    infallible. The Wall, which has no `StructureKind` row by design (`rules.proto` ~666), stops
    reaching `structure_row` at all: its cost is `wall_cost_per_voxel` per voxel (`fire`, `site`).
  - **E1** (live fielding under generational ids, a per-seat beacons-lost counter,
    `SNAPSHOT_VERSION` 10), **E2** (the damage latency rule, landing half: apply-at-end-of-phase,
    the queue never state), **E3**, **E4**, **E5**, **E6**, **E7**, and **E10** (the sensor seam,
    reading vision radii from `con3`'s rows; `UNIT_VISION_RADIUS_VOXELS` deleted, S1-34's sim copy
    discharged).
  - **S2-05's reader** (`HEADROOM_VOXELS` from `con3`'s row) and **S2-06** per decision 24.
  - **The re-dating sweep's sim half** (item 139 (6); `redate` leaves the sim to this lane): the six
    markers in `world.rs`, `rules.rs` (~186, ~191) and `audit.rs` (~34) that name S1's demo,
    re-dated to the ruling that settled each, comments only, shown in the register's before and
    after.
  - **Wording:** `economy.rs`'s two "Base Maintenance Indemnity" (~248, ~266) → "Basic Minimum
    Income"; `voxels.rs`'s "Nothing fills the queue yet" (~274); `knowledge.rs`' and `credit.rs`'
    module docs where E1 changes what a row is.
- **Does not:** fire anything (`fire`); any mandate behaviour beyond ruling 10.
- **Implements:** items 136 (5), 138 (4) (c), 139 (1) rulings 7 and 10, 139 (6), 144 (4); register
  S1-34 (sim), S1-37 (live counts), S2-05, S2-06, S2-07.
- **Needs:** `msrv` merged to start; it **merges after** `con3` (the rows it reads), the merge train
  rebasing it. **Acceptance:** `a_build_list_that_cannot_be_written_whole_writes_nothing` (step
  fails with id 13); `every_paid_target_survives_a_settings_edit` (a ninth target included);
  `a_table_missing_the_economy_block_is_refused`; `a_seat_that_lost_its_units_can_field_again`
  (attrition past the per-seat share); `a_reused_row_does_not_answer_to_its_old_id` (a stale
  kill-credit slot and a stale sighting miss the new asset);
  `beacons_lost_survives_save_and_restore`; `damage_filed_in_combat_lands_in_the_same_tick`;
  `a_negative_damage_cannot_be_built` (a compile-fail doc test or a constructor test);
  `every_dropped_order_is_counted`; `the_queues_cannot_fill_in_a_legal_world`;
  `a_zero_round_limit_is_refused`; `vision_reads_its_row`; **every chain** re-blessed once (the id
  encoding and the beacons-lost column move every one), the first diverging tick named per chain;
  every `report_hash` golden re-blessed (the snapshot version).
- **Contract PR:** yes (determinism code, the snapshot, the event vocabulary). **PLACEHOLDERs:**
  none new; the vision rows' values are `con3`'s. **Agent-days:** 6–7. **Split seam:** ruling 10,
  the refusals and the fallbacks as commit set 1; E1 and E10 as set 2; one PR, re-blessed once.

### `con3` — S2's rules rows and the next `proto/**` pull request's carries

- **Owns:** `proto/gp/v1/rules.proto` and its generated tree, `proto/gp/api/v1/gateway.proto`'s and
  `playbook.proto`'s **comments and the two field changes named below**, `proto/buf.yaml`,
  `rules/rules.v1.json`, `rules/README.md` (after `redate`), the whole of `crates/proto` (its tests
  pin the rules table and the messages S1-42 names), `tests/golden/{proto,docs,verifier}`, and named
  places for the restatements that hold the rules table equal: `godot/scripts/mesher_rules.gd`'s
  `RULES_JSON` (`crates/client-gdext/tests/godot_project.rs`), the gateway goldens that carry the
  table's `revision`. **Branch** `build/s2-rules-rows`, worktree `../pharmakos-con3`, target
  `D:/build/con3`.
- **Builds:**
  - **Rows.** A row that replaces a constant holds today's constant, with decision 17's proposal in
    its owner PLACEHOLDER for `tune2` to apply, so no chain moves; a row nothing reads yet takes
    decision 17's proposal now. Each names the owner at S2's demo: `locomotion.headroom_voxels` = 2
    (S2-05); per-kind `vision_radius_voxels` on `UnitKind` and `StructureKind` (S1-34; every kind 8
    today, the scouts' constant, so the sim's answer is unchanged until `base` reads the rows and
    `tune2` sets them); a `Weapon` message (range, damage, cooldown, splash radius, crater radius,
    shell speed) on the armed kinds — `units.raider`, `structures.autocannon`, `structures.mortar` —
    and on the core (`beacon.core_weapon`); a `Combat` block (the under-attack window for Defend's
    urgency band, the threat-route surcharge, the demolition charge's radius and fuse, the repair
    rate, the terraform rate); a `Collapse` block (`max_region_voxels`, `fall_damage_hp`; item 138
    (3)); and a `Materials` block (the wall's blast levels, the bedrock layer). Field numbers come
    from the free ranges of each message (`UnitKind` 5+, `StructureKind` 4+, `Beacon` 3+,
    `Locomotion` 13+, `RulesTable` 3–9 and 23+); the lane picks them and the reviews read them as a
    contract decision. `revision` 4 → 5.
  - **The proto carries** (item 139 (6), item 136 (5), handoff NEXT 3): `rules.proto`'s comments on
    `lull_ms` ("time every Lull"), `mining_carry_voxels` and `build_hp_per_second` (no constant left
    to swap); `rules.proto:285`'s "Base Maintenance Indemnity" → "Basic Minimum Income";
    `commander.placement_range_voxels` documented as superseded by walk-in (~463); S1-42's
    `crates/proto/src/fingerprint.rs` module doc and the two `proto.rs` test messages (~1027,
    ~1033); `gateway.proto`'s two "for the owner to confirm" markers (S3-23 ~765, S1-46 ~868, ruled
    by item 139 (1) ruling 14) and `BeaconSummary.core`'s S1-13 marker (the `b_00` rule, ~688);
    `playbook.proto`'s "Refused in S1" on `on` and `covering` (~1002, ~1010); `committed_dollars`'
    comment (an upper bound, absent outside a Lull) and `GetEconomyForecastResponse` fields 5 and 6
    made `optional` (item 135 (2) (f), (g)); `ResolvedRef.reachable` added (item 133 (3) (i)).
  - **The re-dating `redate` could not do:** `rules.proto`'s 47 markers (the generated copy
    regenerated), and `rules.v1.json`'s `note` with its golden copy, re-dated in the same rules
    re-bless.
- **Implements:** items 133 (3) (i), 135 (2) (f) (g), 136 (5), 138 (3), 139 (1) ruling 14, 139 (6),
  144 (4), (8); register S1-13, S1-34 (rows), S1-42, S2-05.
- **Needs:** `msrv`, `redate` and `east` merged (`east` re-blesses `tests/golden/proto` and
  `crates/proto`'s tests read it); decisions 10, 11 and 17. **Acceptance:** `buf lint`; `buf
  breaking` in `WIRE_JSON` mode (making fields 5 and 6 `optional` is wire-compatible; any break
  stops the lane); `the_rules_table_is_in_canonical_form`; `the_restated_rows_agree`; every
  `report_hash` golden re-blessed with the reason "`rules_hash` moved: S2's rows added, `revision`
  5, the note re-dated"; the gateway goldens that carry the forecast's two fields re-blessed if
  their JSON moves; `godot_project.rs` green over `mesher_rules.gd`; **no chain moves**; the main
  session's local package run (`rules/` ships).
- **Contract PR:** yes. **PLACEHOLDERs:** every new row's value (owner, S2's demo, with `tune2`'s
  report); the power rows keep item 91's markers (owner, S2's exit, `g3r`). **Agent-days:** 3–4.

### `con4` — S2's playbook and gateway wire

- **Owns:** `proto/gp/v1/playbook.proto`, `proto/gp/api/v1/gateway.proto`, `proto/buf.yaml`, the
  whole of `crates/proto` (after `con3`), `tests/golden/{schema,docs,proto}`, and named places for
  the exhaustive matches and whole-struct literals the new fields break: the sim's
  `compile_settings_row` arms for Defend and Attack (`crates/sim/src/interpreter.rs`; the sim's
  owner of the day, `base` then `fire`, does not edit them while this lane is open), the verifier's
  structure walk for the new messages (`crates/verifier/src/structure.rs`, `walk.rs`),
  `render_plan`'s arms (`crates/plan-core/src/render.rs`), and any `ViewEntity` or `BeaconSummary`
  literal in `crates/gateway` and `crates/client-gdext`. **Branch** `feat/proto-s2-wire`, worktree
  `../pharmakos-con4`, target `D:/build/con4`.
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
    (the licensed blueprints stay S4's); a wall target's box (`Area`) per decision 10.
  - **`BuildSettings` 6 and 7** per decision 15 (recommended: they stay reserved);
    **`MandateSettings` 3 (`report_kinds`)** stays reserved (decision 16).
  - **`gateway.proto`**: `ViewEntity`'s fields from its reserved 6–15 that decision 5 rules (HP,
    heading), the new entity kinds (`shell`, `wreck`, `ruin`), `BeaconSummary`'s enemy detail per
    decision 5 from its reserved 7–15, and `GetRecapResponse`'s kill and loss tally, shared kills
    and value destroyed from its reserved range.
  - **The scoped `buf.yaml` `ignore_only`** for each reserved range discharged, every discharge
    named (AGENTS.md §5).
  - **The new arms are refused until their lanes land:** the sim's compile answers
    `PlanError::NotAtThisStage` naming each construct, the verifier emits that construct's E-code
    (E8; `vfy1` has built the table), and `render_plan` names it plainly.
- **Implements:** items 104, 128 (d), 144 (5); register S2-11 (wire), S2-13.
- **Needs:** `con3` merged; decisions 5, 10, 13, 14, 15 and 16. **Acceptance:** `buf lint`; `buf
  breaking` green with the scoped ignore; the reserved-numbers golden moving for exactly the ranges
  discharged, each named; canonical JSON round-trips for every new message; a refusal test per new
  construct in the sim and the verifier; `get_schema` and `gamectl docs` goldens re-generated; no
  chain and no `report_hash` moves.
- **Contract PR:** yes. **PLACEHOLDERs:** none. **Agent-days:** 3–4.

### `rnds` — `crates/client-gdext` + `godot/`: `-- --rounds <n>` (item 143 (5))

- **Owns:** `godot/scripts/host_link.gd` and its pins in
  `crates/client-gdext/tests/godot_project.rs`. **Branch** `feat/client-rounds-option`, worktree
  `../pharmakos-rnds`, target `D:/build/rnds`.
- **Builds:** `host_link.gd` reads `--rounds <n>` from `OS.get_cmdline_user_args()`, beside its
  `--gamectl=` and `--root=` readers (~98, ~118), and writes the argument's text into the config
  line's round-limit field in place of `ROUND_LIMIT` (~74, ~134–135); absent, the line keeps 3. The
  client parses and bounds nothing: a value that is not a whole number, is 0 or does not fit a `u32`
  is the gateway's refusal (`serve::Config`, `host.rs` ~288), which the lobby draws as it draws any
  host that refuses its config line (E21). The only thing the client refuses itself is an argument
  holding a tab or a newline, because that would change the line's framing rather than its value.
  The option is not shown in the lobby. The config line's format and `last_match.txt` are unchanged
  (AGENTS.md §5).
- **Implements:** item 143 (5). **Needs:** nothing merged. **Acceptance:** `godot_project.rs` pins
  the reader and the unchanged line format; `watch_check.gd` run by hand with a refused value
  (`--rounds 0`) showing the gateway's refusal; the PR names the reader for AGENTS.md §3 rule 4's
  list (section 4.4). **Contract PR:** no. **Agent-days:** 0.5–1.

### `vfy1` — `crates/verifier`: S2's first verifier lane, the carries

- **Owns:** `crates/verifier`, `tests/golden/verifier`, and the named place in
  `godot/fixtures/rows_report.json` if its four goldens move. **Branch** `feat/verifier-s2-carries`,
  worktree `../pharmakos-vfy1`, target `D:/build/vfy1`.
- **Builds:** S2-14, a verifier E-code for a seal whose `$`/`kW` projection overflows the sim's
  types or the forecast's `int32` (what `Surface::seal_commitment` refuses, item 135 (2) (d));
  S2-15, an E-code for `scout_count` > 255 (what `PlanError::ScoutsOutOfRange` refuses); E8's
  refusal table and its cross-check test over the sim's `NotAtThisStage` table; `initial_rows`
  brought to the sim's order (item 136 (3) (r)); `hash.rs`'s `append_bytes` (~151), whose
  `unwrap_or(u32::MAX)` becomes a checked length with a typed error; S1-51's stale docs in
  `interface.rs` (~18, ~366, `count`'s); `limits.rs`' reach window dated S3 (~203).
- **Implements:** items 135 (2) (d), 136 (3) (r), (5); register S1-51, S2-14, S2-15. **Needs:**
  `msrv` merged. **Acceptance:** a case for each new code (`every_emitted_code_has_a_case`);
  `every_construct_the_sim_refuses_has_an_e_code`; the catalogue golden; `report_hash` goldens only
  if `append_bytes`' change moves an encoding (it should not: no real length reaches `u32::MAX`).
  **Contract PR:** no. **Agent-days:** 3–4.

### `night` — `crates/gamectl`, the nightly harness and its workflow (S2-01, M-01)

- **Owns:** `crates/gamectl`, `xtask/src/scenario.rs`, `.github/workflows/nightly-scenarios.yml`,
  the new `scenarios/nightly/` (its README, `seeds.txt` and three placeholder arms that `arms`
  replaces), and `scenarios/README.md`'s layout section (after `east`). **Branch**
  `feat/gamectl-nightly`, worktree `../pharmakos-night`, target `D:/build/night`.
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
    golden; a `gamectl` test verifies each arm's playbook (it must qualify) and plays each arm one
    round on its own seed to the round's end with no refusal.
  - **Carried:** `gamectl`'s `b_01`-as-core texts (`scenario/run.rs` ~484, `seat.rs` ~27,
    `strings.rs` ~293) renamed to `b_00`.
- **Implements:** item 116 (6) (j); item 144 (6); register S2-01, M-01. **Needs:** `msrv` merged
  (one `xtask`/`.github` PR at a time: none other is open); decision 29. **Acceptance:** CLI tests
  for every flag, its refusal elsewhere and an unknown operator; the seed decoder's property test; a
  result golden for a two-seed run of a placeholder arm; `win_rate_percent` rounding pinned; the
  workflow run once through its `force` input on the branch and its artifact attached to the PR; the
  G4 job's compare green on that run. **Contract PR:** yes (`xtask`'s scenario step, a workflow).
  **Agent-days:** 4.5–5.5.

### `fire` — `crates/sim`: weapons, shells, materials, wrecks and the kill-credit split

- **Owns:** `crates/sim` (except `con4`'s named place while it is open), every golden its changes
  move, the named places of section 3's preamble, and its scenarios' entries. **Branch**
  `feat/sim-combat`, worktree `../pharmakos-fire`, target `D:/build/fire`.
- **Builds:**
  - **Firing** (E2's choosing half, E12): the autocannon, the core's built-in autocannon (fed by the
    bore, never shed in a brownout, drawing no `kW`; item 123; spec §5), the mortar and the raider's
    gun, each reading its `Weapon` row; range tests on squared distance (AGENTS.md §4.2); cooldowns
    in game milliseconds as hashed state of the armed assets; targets per decisions 8 and 21.
  - **The rules of engagement** for every armed asset: `HOLD_FIRE` never fires, `RETURN_FIRE` fires
    only at an asset that damaged one of the seat's own assets inside its range within the
    under-attack window, `FREE_FIRE` fires at any enemy in range (decision 4 states these readings
    with the unset case). `retreat_hp_pct` per decision 4. Return fire and retreat are announced
    (spec §6's Common row): an event each.
  - **Shells** (E13) and **splash**: a landing shell damages every asset within its splash radius,
    the firer's own included (friendly fire, spec §9), and files a crater edit carrying its seat.
    **Friendly-fire avoidance in aim:** a mortar never chooses a landing point whose splash radius
    holds an own unit, structure, beacon or own wall voxel (spec §9: "the built-in mandates aim to
    avoid your own assets"); a shell that lands on an own asset that walked under it still hurts it.
  - **Materials** per decision 10 and E14: wall materials by seat and damage level; a blast removes
    terrain inside its crater radius, steps a wall voxel down one level, and never removes bedrock
    or the core's bore. A crater that removes a vent's voxels loses the vent, which the feature
    table's derived liveness already reads (AGENTS.md §4.8). `VoxelEdit` carries the editing seat
    (`Option<SeatId>`, `None` for a seat-less edit), which `fall` reads for credit.
  - **Deaths:** wrecks (E16); the per-seat destroyed value (E15), the audit's term and its first
    tie-break read as decision 7 rules (X-08; `audit.rs`' marker discharged); kill-credit settlement
    exercised at a realistic death rate (G3′ §9.12).
  - **The threat value** of an armed asset, its weapon's damage per second from its row (decision
    12), as a pure function `dfnd` and `atk` read.
  - **Events** (E17): `unit_destroyed`, `structure_destroyed`, `shell_landed`, `wreck_left`,
    `returned_fire`, `retreat_announced`. No event per shot: the bus holds 512 events between two
    drains (`events.rs`), and shots are counted in the unhashed work counter instead.
  - **S2-09** (E11). **Scenarios:** `scenarios/s2/mortar-breaks-wall` (a mortar shells a wall to a
    gap; craters; wall levels) and `scenarios/s2/raid-on-a-mortar` (raiders dive a mortar, the
    autocannon and the core's kill them, wrecks left), each asserting events and hashes.
- **Does not:** Build's placement of the new blueprints (`site`), collapse (`fall`), Defend and
  Attack (`dfnd`, `atk`). Its tests place armed structures and units through test seams.
- **Implements:** items 17, 123, 144 (8); spec §3 (kill credit, the audit), §5 (the core), §9;
  register S2-09, X-08.
- **Needs:** `base` and `con3` merged; `con4` merged before it lifts any refusal; decisions 4, 7, 8,
  10, 12 and 21. Its `strings.rs` arms are its last commit, after `wgw` merges.
- **Acceptance:** `an_autocannon_kills_a_raider_in_range_and_not_out_of_it`;
  `return_fire_waits_for_damage`; `hold_fire_never_fires`; `fire_is_simultaneous` (two assets that
  kill each other on one tick both die); `a_shell_misses_a_unit_that_moved`;
  `splash_hurts_the_firers_own_unit`; `a_mortar_never_aims_where_its_splash_holds_an_own_asset`;
  `a_wall_voxel_breaks_on_its_last_level`; `bedrock_and_the_bore_survive_any_blast`;
  `a_crater_that_takes_a_vent_kills_its_feature`;
  `a_death_leaves_a_wreck_worth_the_salvage_percent`;
  `destroyed_value_is_the_settled_share_of_build_cost`; `the_shell_table_survives_save_and_restore`;
  `the_combat_stream_is_drawn_from_nowhere_else`; the two scenarios; every chain re-blessed (the new
  per-seat and per-asset columns; the core's cooldown in every match), the first diverging tick
  named; every `report_hash` golden (`SNAPSHOT_VERSION` 11).
- **Contract PR:** yes (determinism code: the combat phase, the first `Stream::Combat` draw, the
  snapshot, the event vocabulary). **PLACEHOLDERs:** none new. **Agent-days:** 9–10.5. **Split
  seam:** direct fire, the rules of engagement, deaths, wrecks and destroyed value as set 1; shells,
  splash, materials and craters as set 2; one PR, re-blessed once.

### `wgw` — `crates/gateway`: S2's surfaces

- **Owns:** `crates/gateway`, `tests/golden/gateway`, and the byte copies under `godot/fixtures/`
  its goldens move. **Branch** `feat/gateway-combat`, worktree `../pharmakos-wgw`, target
  `D:/build/wgw`.
- **Builds:** S2-11 per decision 5 in `get_view`'s entities and `BeaconSummary`; E9 (the prose
  follows the summary); S1-34's gateway copies (`surface/watch.rs` `sight_key`, `viewfeed.rs`
  `Sight`, `host.rs` `SphereVision`, ~636) reading the sim's sensor seam's live-view output instead
  of restating spheres; the recap's kill and loss tally, shared kills and value destroyed; feed
  lines and audiences for `base`'s `StepFailure` 13 and `fire`'s kinds if those lanes have not had
  the place; the `seal_commitment` door's test (item 136 (5)); `host.rs`'s `SAFE_PLAYBOOK` marker
  (~103) reworded to decision 6 (the gateway's own fallback, for a seat no operator advises, still
  writes nothing); after `vfy2` merges, `verifier_scope`'s sightings for Attack's freshness (the
  seat's own sightings of enemy beacons and of the enemy commander, with their ages; the S1 pattern
  of `tgtw` filling what `tgtv` laid out); S2-12's measurement (decision 27): the bytes of a full
  entity list at the gate load of 300 units, recorded in the PR.
- **Implements:** items 104 (the gateway half), 136 (5); register S1-34 (gateway), S2-02 (gateway's
  marker), S2-11, S2-12 (measured). **Needs:** `con4` and `base` merged; decisions 5, 6 and 27.
  **Acceptance:** `an_enemy_beacons_prose_says_what_its_summary_says`;
  `the_view_shows_what_decision_5_rules` with a fogged and a visible enemy;
  `the_recap_names_a_shared_kill`; `the_seal_commitment_door_refuses_an_unpriced_seal`; method,
  walkthrough and view goldens moved and explained; the gateway's `report_hash` goldens re-blessed
  when the scope's sightings land. **Contract PR:** no (a `report_hash` input's content; `vfy2`
  changes the layout). **Agent-days:** 5–6.

### `vfy2` — `crates/verifier` + `crates/plan-core`: S2's constructs

- **Owns:** `crates/verifier`, `crates/plan-core`, their goldens, the named place in
  `godot/fixtures/rows_report.json`, and, if it changes the verifier goldens' fixture, the named
  place in `crates/bench/src/quick.rs` (item 131 (5)). **Branch** `feat/verifier-s2`, worktree
  `../pharmakos-vfy2`, target `D:/build/vfy2`.
- **Builds:** structure and semantic checks for `DefendSettings`, `AttackSettings`, the wall box and
  the new blueprints (ranges, legal places, the go-code arm refused as S4's with its E-code);
  interface pricing for the new fields at decision 15's (S1) one-field-per-element rule
  (`interface.rs` ~417–424 stop answering zero), and the projection's `$` for a wall's voxels, a
  demolition charge and the new structures' `kW` (`projection.rs` ~536–541); Attack's plan-time
  freshness test (spec §6: "Freshness is a qualification test at plan time only; the target is
  pinned at the seal"): `Scope` gains the seat's sightings with their ages (a `report_hash` layout
  change, the contract half of `wgw`'s fill), and an Attack target unknown or older than
  `verifier.reach_memory_ms` (`rules.proto` ~728) is an error with its own code; W0703's emitter
  (the reach window) where it reads the same sightings; `render_plan` prose and canonical form for
  every new construct; E8's codes for the constructs `con4` refuses. The verifier never ranks and
  never models enemy behaviour (AGENTS.md §3 rule 2): freshness reads ages, nothing else.
- **Implements:** items 13, 104 (the verifier half); spec §6, §11. **Needs:** `con4` and `vfy1`
  merged; decisions 10, 13, 14 and 15. **Acceptance:** a case per new code;
  `full_finds_what_quick_finds`; `the_verifier_never_ranks` unchanged; render and canonical goldens
  for each new construct; every `report_hash` golden re-blessed (the scope's layout). **Contract
  PR:** yes (`report_hash`'s scope encoding). **Agent-days:** 5–6.

### `site` — `crates/sim`: Build's S2 settings, the base kit's blueprints, repair and salvage

- **Owns:** `crates/sim` (except `fall`'s named place), every golden it moves, the preamble's named
  places, its scenarios' entries. **Branch** `feat/sim-build-s2`, worktree `../pharmakos-site`,
  target `D:/build/site`.
- **Builds:**
  - **The base kit's blueprints:** `autocannon`, `mortar` and `wall` as Build targets, the wall a
    box of voxels built one voxel at a time at `wall_cost_per_voxel` (decision 10), the others at
    their rows.
  - **Terraform** (item 144 (5)): before building, a drone levels the target's footprint to its
    anchor's height — `FILL` raises, `DIG` lowers, `BOTH` both — each voxel at the wall's cost per
    voxel; `FILL` and `BOTH` also refill craters and trenches inside the beacon's sphere back to the
    generated surface, each voxel through `set` (AGENTS.md §4.8). The generated surface is a derived
    per-column table, a pure function of the seed and the rules made with the map, regenerated by
    `Snapshot::restore_into` and never hashed per tick, as the feature table is (AGENTS.md §4.8),
    and pinned by a new `tests/golden/mapgen` file (a golden format, so a contract path). Refill
    competes with building for drone time under the mandate's order.
  - **Repair threshold** and **rebuild destroyed** (item 128 (d)): a target below the threshold is
    "damaged" for "highest-order affordable target that is unbuilt or damaged"; a destroyed ordinary
    target is rebuilt when `rebuild_destroyed` is set; a capability structure never is (spec §8).
  - **Repair/reclaim drones** (spec §9): fabricated by Build beacons on work in hand (a damaged own
    structure, a wounded commander in the sphere, a wreck or ruin in the sphere), they repair any
    own structure inside the sphere and the commander (spec §4: no regeneration, repair drones heal
    it) at the repair row, then salvage wrecks and ruins of any seat inside the sphere for `$`.
  - **S2-10** per decision 26.
  - **Scenarios:** `scenarios/s2/repair-and-salvage` and `scenarios/s2/terraform-refills-a-crater`.
- **Implements:** items 128 (d), 144 (5); spec §6's Build row, §8, §9; register S2-10.
- **Needs:** `fire` merged; decisions 10, 15 and 26. **Acceptance:**
  `fill_raises_a_footprint_to_its_anchor`; `dig_lowers_it`;
  `fill_refills_a_crater_to_the_generated_surface`; `none_leaves_ground_as_it_is`;
  `each_levelled_voxel_costs_the_walls_price`; `a_target_below_its_repair_threshold_is_damaged`;
  `rebuild_destroyed_rebuilds_an_ordinary_target_only`; `a_repair_drone_heals_the_commander`;
  `any_seats_wreck_is_salvageable_inside_the_sphere`; `a_wall_box_is_priced_per_voxel`; the two
  scenarios; chains with a Build re-blessed with reasons (the determinism chain's Hold & Build
  writes `terraform` omitted, NONE, so it should not move: if it does, the PR says why);
  `SNAPSHOT_VERSION` 12 if drone jobs add state, every `report_hash` with it.
- **Contract PR:** yes in part (hashed drone state, the event vocabulary). **Agent-days:** 7–8.
  **Split seam:** blueprints, repair and salvage as set 1; terraform and S2-10 as set 2.

### `fall` — named place in `crates/sim`: the support rule (item 138 (3), item 144 (1) to (3))

- **Owns:** a new `crates/sim/src/support.rs` (which holds its own `impl World` block, so `world.rs`
  gains one call line), that call line in `phase_voxels`, the dig check's named lines in
  `crates/sim/src/mining.rs`, `crates/sim/tests/support.rs`, a collapse-every-tick case in
  `crates/sim/tests/allocations.rs`, its scenario's entries, and the preamble's named places for its
  events. It runs beside `site` (AGENTS.md §6 named place). **Branch** `feat/sim-support-rule`,
  worktree `../pharmakos-fall`, target `D:/build/fall`.
- **The design.** The rule sits behind one interface, `support::Collapse`, with one method the
  Voxels phase calls — given this tick's removed voxels, return the regions that fall and how far —
  so that physics (item 138 (3)'s roadmap entry) replaces the implementation and not its caller.
  - **Support.** A solid voxel is supported when a face-connected path of solid voxels joins it to
    the bedrock layer or to the core's bore, which is never detachable. A region is a face-connected
    set of solid voxels; a region with no such path is detached.
  - **Incremental.** Only voxels that might have lost support are examined: the seeds are the solid
    face-neighbours of every voxel an edit turned to air this tick (craters, `set` to air, digs),
    deduplicated.
  - **Total order.** Seeds are sorted by voxel index (the chunk store's linear index). Each seed not
    yet labelled starts a breadth-first search over solid face-neighbours, enumerated in a fixed
    order (−z, +x, −x, +y, −y, +z). A search ends **supported** the moment it reaches a voxel whose
    own column is solid down to bedrock (a per-column "solid from the bottom" height, read from the
    store and cached for the tick's touched columns, which makes ordinary ground next to a crater an
    O(1) answer), a bore voxel, or a voxel already labelled supported. It ends **detached** when its
    frontier empties. A detached region is numbered by its lowest voxel index, and regions fall in
    that order.
  - **Bounded.** A search that visits more than `collapse.max_region_voxels` voxels without
    grounding is labelled supported: a mass that large is wedged into the ground around it, which is
    the rule's in-world reason and the technical bound that keeps the worst case inside the tick
    (decision 11). The bound is a rules row, so its worst case — one crater cutting off a region of
    exactly that size — is a number G3′ measures (`g3h`), not only a mean.
  - **The fall.** A detached region drops rigidly by the smallest air gap under any of its voxels,
    every voxel leaving and returning through `set`, which marks its chunk (AGENTS.md §4.8). Every
    unit, structure and beacon whose standing voxel or anchor column is in the region moves down
    with it and takes `collapse.fall_damage_hp` once (item 144 (1)); an asset under the landing
    voxels is handled per decision 11. The damage is filed with the seat whose edit produced the
    region's first seed (the lowest-index seed of the region; `VoxelEdit`'s seat, `fire`'s field),
    so kill credit goes to that seat as for direct fire, and to nobody when the asset is that seat's
    own or the edit had no seat (item 144 (2); spec §3, self and terrain losses credit nobody). It
    applies at the end of the Voxels phase (E2).
  - **Place in the tick.** Inside `phase_voxels` (phase 10 of 12, after Pathing): apply the queued
    edits; run the support search over this tick's removals; drop each detached region in order;
    then settle the chunk digests once and refresh the pathing surface and mark the clusters of
    every settled chunk, as today (`world.rs` ~3109–3143). The falls therefore reach the hash in the
    same tick and the pathing graph through the existing path.
  - **The HPA\* cluster cache.** Collapse adds no invalidation path of its own: every voxel it moves
    goes through `set`, its chunk is settled, and `mark_chunk_clusters` marks that chunk's clusters
    for the next tick's Pathing repair (`pathing/repair.rs`), with the one-tick lag every edit has.
    A region spanning chunks marks each. A unit whose route crosses a changed column repaths through
    the router's existing check, under the cap of 16.
  - **Allocations.** None in the tick: a one-bit-per-voxel visited set over the whole map (about 1.2
    MB at 384 × 384 × 64, allocated once at construction and cleared by walking the list of bits
    each search set), a FIFO queue and a region list each sized from `max_region_voxels` at load, a
    per-column height cache sized to the map's columns, and the seed list sized from the edit
    queue's worst case (E5). `crates/sim/tests/allocations.rs` gains a collapse every tick.
  - **Mine never undermines** (item 144 (3)): before a dig, the mining program runs the same bounded
    search read-only with the voxel treated as air; if any region it would detach carries a seat's
    structure or beacon, the voxel is refused and the drone picks the next. Pillar spacing counts as
    support for free: an undug column is solid to bedrock.
- **Builds:** the design above; events `region_collapsed` and `asset_fell` (E17); scenario
  `scenarios/s2/collapse-credits-the-blast`.
- **Implements:** items 138 (3), 144 (1) to (3). **Needs:** `fire` merged (`VoxelEdit`'s seat, wall
  materials); decision 11. **Acceptance:** `a_region_cut_off_by_a_crater_falls`;
  `ground_beside_a_crater_is_supported_in_one_step`;
  `regions_fall_in_the_order_of_their_lowest_voxel`; `a_region_larger_than_the_bound_holds`;
  `the_bore_never_falls`; `everything_on_a_falling_region_falls_and_takes_the_flat_damage_once`;
  `the_crater_seat_gets_the_kill_credit_and_an_own_crater_credits_nobody`;
  `mine_never_digs_a_voxel_that_would_drop_a_structure`; `pillars_hold_a_roof`;
  `collapse_allocates_nothing_in_the_tick`; the scenario; chains re-blessed only if a committed run
  detaches a region (none is expected; one that does is named); no snapshot change (the rule holds
  no state between ticks).
- **Contract PR:** yes (determinism code: the Voxels phase, the event vocabulary). **PLACEHOLDERs:**
  none new (the constants are `con3`'s rows). **Agent-days:** 5–6.

### `view` — `crates/client-gdext` + `godot/` + `crates/mesher`: drawing combat

- **Owns:** `crates/client-gdext`, `godot/`, `crates/mesher`, their goldens. **Branch**
  `feat/client-combat`, worktree `../pharmakos-view`, target `D:/build/view`.
- **Builds:** HP on own assets and on visible enemies as decision 5 rules, from the view's fields;
  shells, wrecks and ruins as entities; a unit's heading where the view carries it; the Attack
  beacon's target drawn in the editor (`editor.rs` ~176); the remesh under a crater stream: today
  more than 16 changed chunks re-bakes the whole map (`view.rs` `WHOLE_BAKE_ABOVE`, ~297, ~635) and
  one edit's light pad of 15 dirties more chunks than G1 measured (`mesher/src/bake.rs` ~35–45), so
  the lane measures a 20 and a 40 explosions-a-second stream through the mesher's headless CPU proxy
  (G1's own stress step, which "S2 will need") on all three runners and replaces the whole re-bake
  with a bounded incremental relight if the proxy shows it misses G1's three frames; item 136 (5)'s
  `NOT_FOUND` narrowing decided and `editor.rs`'s doc brought to it; `client-gdext/src/rules.rs`'s
  stale comment (~283). Every new piece that schedules or draws without deciding is named in the PR
  for AGENTS.md §3 rule 4. Nothing here decides, times or validates (rule 4).
- **Implements:** register S2-12's measurement; G1's stress headroom. **Needs:** `wgw` merged.
  **Acceptance:** vista and rows PNGs re-rendered on CI's Linux leg and looked at;
  `godot_project.rs` pins; the mesher's perf alarm extended to the stream; the watch check by hand;
  the main session's local package run. **Contract PR:** no. **Agent-days:** 5–6.5.

### `g3h` — `crates/bench` + `xtask`: the G3′-real harness

- **Owns:** `crates/bench`, `xtask/src/main.rs`'s `perf_alarms` entries, and a named place in
  `crates/sim`: a phase-boundary seam the walled harness can time from outside (the sim has none on
  `main`; the seam reads no clock itself, AGENTS.md §4.5, and is reached only from `crates/bench`,
  which a test holds), edited only while `fall` is not open beside `site`, so that no third agent is
  in the sim. **Branch** `feat/bench-g3-real`, worktree `../pharmakos-g3h`, target `D:/build/g3h`.
- **Builds:** section 5's harness: a hosted match at the gate load (decision 2's seats, 300 units
  and 40 beacons as world totals, item 63), with real combat, real pathing (no charged substitute;
  item 64: G2's CSR rebuild counted once) and a crater stream at the destruction rate the budget is
  set with; per-phase tick timings, the tick's p50, p99 and mean, ticks a second, the
  tick-minus-pathing mean (item 116 (6) (b)), the repath backlog against the cap, the broadphase's
  share, the support search's worst tick at a region of `max_region_voxels`, kill-credit settlement
  at the run's death rate, and live heap bytes over a full 8-minute segment through a counting
  global allocator (item 68; std-only, so no dependency); every figure published by `cargo xtask
  perf-alarms` as notices; `tests/harness.rs`' doc list of restated items extended (item 133 (5)).
- **Implements:** items 63, 64, 68, 69, 116 (6) (b), 138 (3) (the worst case). **Needs:** `base`
  merged to start, driving walkers and the host's damage seam; it rebases onto `fire` (merged by day
  17) before it opens its PR, and `g3r` re-points it at the finished sim once `atk` merges; decision
  2. **Acceptance:** `wall-guard` and the research guard green; the notices on all three runners.
     **Contract PR:** yes (`xtask`). **Agent-days:** 4–5.

### `dfnd` — `crates/sim`: Defend, the threat model, the bodyguard

- **Owns:** `crates/sim` (except `surv`'s named place), every golden it moves, the preamble's named
  places (the Defend and `beacon_under_attack` rows of `mandate_and_rule_gaps()`), its scenarios'
  entries. **Branch** `feat/sim-defend`, worktree `../pharmakos-dfnd`, target `D:/build/dfnd`.
- **Builds:** the threat model per decision 12 (`most_threatened`, `beacon_under_attack`, the Defend
  urgency band's trigger, `safest`'s threat term (S2-04), `SAFEST` as a seam choice,
  `flee_on_threat` for Mine); the Defend mandate: threat-weighted targeting inside the engagement
  radius, pursue as set, the protect list with `my_commander` (guards escort the commander and
  intercept threats near it while it is inside the sphere), damaged units falling back to the
  nearest friendly repair drone, the picket (scouts patrolling just outside the sphere edge, through
  the sensor seam); raiders fielded as guards on work in hand (a threat seen in the sphere);
  decision 13's deferrals refused with their E-codes. Defend's state (guard assignments, picket
  legs) is a hashed table of its own with a row per Defend beacon, so a match with none hashes as
  before. Scenario `scenarios/s2/guards-escort-the-commander`.
- **Implements:** items 104, 138 (4) (c) (pickets); spec §6's Defend row, §4's bodyguard; register
  S2-04. **Needs:** `site` merged; decisions 12 and 13. **Acceptance:**
  `the_most_threatened_beacon_holds_the_most_threat`; `under_attack_reads_its_window`;
  `guards_escort_the_commander_inside_the_sphere_only`; `a_damaged_guard_walks_to_a_repair_drone`;
  `pursue_never_stops_at_the_engagement_radius`; `a_picket_sees_past_the_sphere`;
  `safest_seam_avoids_a_known_threat`; `a_mine_drone_flees_a_threat`; the gateway's two door rows
  flipped; W0501's warning lifted in the verifier through the named place; the scenario; chains
  re-blessed if any committed run reads `safest` with a threat now in view (`against-easy*` may);
  `SNAPSHOT_VERSION` bumped for Defend's state.
- **Contract PR:** yes (determinism code, the snapshot). **Agent-days:** 6–7.

### `surv` — named place in `crates/sim`: full Survey

- **Owns:** `crates/sim/src/survey.rs`, the Survey half of `crates/sim/src/sight.rs`, a new
  `crates/sim/tests/survey.rs`, the per-seat explored-ground table's hash and snapshot lines (named
  lines in `hash.rs` and `snapshot.rs`, coordinated with `dfnd` at the merge), its scenario's
  entries. Runs beside `dfnd`. **Branch** `feat/sim-full-survey`, worktree `../pharmakos-surv`,
  target `D:/build/surv`.
- **Builds:** spec §6's Survey row: scouts scan unexplored ground inside the probe areas first (a
  per-seat explored table over coarse cells, hashed and snapshotted); then re-check the sightings
  closest to leaving the reach window, enemy beacons first, refreshing them before the next Lull;
  avoiding known threats (decision 12); reporting back through the feed. Scenario
  `scenarios/s2/survey-finds-the-enemy`.
- **Implements:** spec §6's Survey row; item 138 (4) (c). **Needs:** `fire` merged; decision 12.
  **Acceptance:** `scouts_scan_unexplored_ground_first`; `the_stalest_sighting_is_refreshed_next`;
  `enemy_beacons_come_first`; `the_explored_table_survives_save_and_restore`; the scenario; chains
  re-blessed (the per-seat table moves every chain once). **Contract PR:** yes (hashed state).
  **Agent-days:** 3–4.

### `vfy3` — `crates/verifier`: the threat model's lints

- **Owns:** `crates/verifier`, its goldens. **Branch** `feat/verifier-threats`, worktree
  `../pharmakos-vfy3`. **Builds:** W0501 retired with its catalogue row (SAFEST means something
  now), if `dfnd` has not done it in its named place; an I-level note where `AVOID_KNOWN_THREATS`
  has no sighting in `Scope` to avoid and so routes as `DIRECT` (a new code, the editor's
  explanation and no more); the prose and catalogue text for SAFEST and `flee_on_threat` brought to
  decision 12; E0209 stays S3's (the enemy-knowledge predicates). **Needs:** `dfnd` and `vfy2`
  merged. **Acceptance:** a case per code; the catalogue golden. **Contract PR:** no.
  **Agent-days:** 1.5–2.

### `atk` — `crates/sim`: Attack, hunting, breach and the demolition charge

- **Owns:** `crates/sim`, every golden it moves, the preamble's named places (the Attack row of
  `mandate_and_rule_gaps()`), `crates/sim/tests/allocations.rs`, its scenarios' entries. **Branch**
  `feat/sim-attack`, worktree `../pharmakos-atk`, target `D:/build/atk`.
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
  - **Breach** (E23): with `breach_allowed`, an Attack whose route is sealed by walls queries the
    pathfinder's breach mode for the cheapest way through, sends its mortars' fire or a raider
    carrying a demolition charge (decision 9) to the first wall column on that route, and resumes
    once the gap opens; the charge is bought at `demolition_charge_cost_dollars` when it is loaded,
    planted on arrival, and detonates when its fuse row runs out as a crater of its radius row, with
    the carrier's seat on the edit. Intercepting the carrier is the counter: a carrier killed before
    it plants drops a charge that never fires (decision 9).
  - **Attack's state** (the pinned target, the staging and launch state, the wave count, a charge's
    carrier and fuse) in a hashed table of its own with a row per Attack beacon, as Defend's is.
  - **A moving crowd's allocations** (item 137 (2) (j)): `allocations.rs` gains an attacking crowd
    under combat, and its fixed-stride check reads the live-row tables E1 made dense.
  - **Scenarios:** `scenarios/s2/hunt-the-commander` and `scenarios/s2/breach-and-eliminate` (a
    breach, a beacon destroyed into ruins, the last beacon lost, the match ending
    `last_seat_standing` on that tick).
- **Implements:** items 13, 104; spec §4 (hunting), §6's Attack row, §9 (breach, friendly fire);
  register S2-03. **Needs:** `dfnd` merged; decisions 9 and 14. **Acceptance:**
  `an_attack_waits_for_its_force_size`; `the_target_is_pinned_at_the_seal`;
  `a_hunted_commander_is_retargeted_while_seen_and_lost_when_not`;
  `avoid_known_threats_routes_around_range`; `a_breach_sends_a_charge_to_the_wall_and_resumes`;
  `a_carrier_killed_before_it_plants_fires_nothing`; `a_charge_crater_carries_its_seat`;
  `the_last_beacon_lost_ends_the_match_that_tick`; the gateway's door row flipped; `allocations.rs`
  green with the crowd; the two scenarios; chains re-blessed where Easy's playbooks now attack (none
  of the committed ones is expected to; any that moves is named); `SNAPSHOT_VERSION` bumped for
  Attack's state.
- **Contract PR:** yes. **Agent-days:** 8.5–10. **Split seam:** the mandate, the pin and hunting as
  set 1; breach, the charge, avoid areas and waves as set 2.

### `oprd` → `opra` — `crates/operator` + `library/`: Easy under combat, one lane in two PRs

- **Owns:** `crates/operator`, `library/`, `tests/golden/operator`, and the named places AGENTS.md
  §6 grants a lane that changes the operator's output: `crates/gamectl/tests/{operator.rs,
  operator_host.rs, wizard.rs}`, `crates/gateway/tests/advice.rs`,
  `crates/client-gdext/tests/wizard.rs`, the gateway's `instantiate_suggested` golden and
  `godot/fixtures/instantiate_suggested.json`. **Branches** `feat/operator-defend` then
  `feat/operator-attack`.
- **`oprd` builds** (after `dfnd`): the Defend template in `library/` (guard the core and "my
  commander"); Easy's defence score from the threat model, a Defend guard on its commander when the
  commander died recently (spec §14); S2-02's rescue rule per decision 6 in the safe playbook and
  Easy; `wire.rs`' defaults (`int_of`, `text_of`, `bool_of`, `array_of`, `voxel`) and
  `candidates::score`'s `time_ms.max(1)` made typed reads (item 136 (5)); Expand & Mine's "ore seam"
  → "scrap seam" (spec §9). 3 ad.
- **`opra` builds** (after `atk`): the Attack template (raid the nearest known enemy beacon); Easy's
  attack score (`candidates.rs` ~54–55, "aiming at an enemy is S2's") and enemy-commander raids
  scored like any other target (spec §14); the wizard's suggestions under both templates. 2 ad.
- **Implements:** spec §14 (S2's share); register S2-02. **Needs:** `oprd`: `dfnd` merged, decision
  6; `opra`: `atk` merged. **Acceptance:** the templates round-trip, verify and render identically;
  the operator goldens, the wizard's and `instantiate_suggested` moved and explained;
  `against-easy*` re-blessed when Easy's sealed files change; the main session's local package run
  (`library/` ships). **Contract PR:** no. **Agent-days:** 4.5–5.5 in all.

### `arms` — the three nightly arms and their bands

- **Owns:** `scenarios/nightly/` (after `night`). **Branch** `feat/scenarios-nightly-arms`, worktree
  `../pharmakos-arms`.
- **Builds:** Rusher (an early Attack beacon, launch on a small force size against the nearest known
  enemy beacon, then the core), Turtle (Defend beacons on the core and "my commander", walls and
  autocannons, repair drones, hoarding), Hunter (Survey for the commander, an Attack targeting
  `enemy_commander`), each a hand-written playbook in v1 vocabulary (spec §15), checked by `night`'s
  per-PR test; `seeds.txt` with twenty seeds, the balance check's twenty (S1-53, item 139 (1) ruling
  5), so the two reports share maps, committed and never regenerated; the written bands in
  `scenarios/nightly/README.md` per decision 29; a forced nightly run on its branch, its report
  attached; `demo2` forces it again on `tune2`'s numbers.
- **Implements:** spec §15; item 144 (6); AGENTS.md §10 item 5. **Needs:** `night`, `atk` and `opra`
  merged. **Acceptance:** each arm qualifies; the forced run's three results; every band written.
  **Contract PR:** no. **Agent-days:** 2–3.

### `g3r` — the certifying run, the gate job and the re-derived power rows

- **Owns:** `crates/bench` (after `g3h`), `xtask/src/main.rs` (`g3-budget`),
  `.github/workflows/ci.yml` (the new job and the perf-alarms comment, ~986). **Branch**
  `feat/g3-real-gate`, worktree `../pharmakos-g3r`.
- **Builds:** the harness re-pointed at the finished sim (`atk` merged); the certifying run on the
  owner's machine (i7-9800X) recorded in the PR, with the three runners' notices beside it; E18's
  command and job; the re-derived inputs for `tune2`, every one stated: the per-map power budget and
  the three power rows that move together (items 65, 91), the destruction rate set with them, the
  repath cap from the measured burst frequency (item 69's window 15–16, floor 9.45), the broadphase
  cell size, the fielding totals (S1-37, S2-08) and `max_region_voxels` if its worst case does not
  fit; G4's bar's first nightly result cited. If G3′ misses, the written fallback (section 5).
- **Implements:** items 63–69, 91, 116 (6) (b), 138 (3); spec §16's G3′ row; AGENTS.md §9 item 11.
  **Needs:** `atk` and `g3h` merged; decision 2. **Acceptance:** the job green on its branch; the
  recorded figures; the proposed rows handed to `tune2` in the PR body. **Contract PR:** yes
  (`xtask`, a workflow). **Agent-days:** 2–3.

### `tune2` — the numbers, the power budget and the walkers

- **Owns:** `rules/rules.v1.json` values and `rules/README.md`; `crates/gamectl/tests/balance.rs`
  and a new `#[ignore]`d `crates/gamectl/tests/duels.rs`; S2-16's walker removal as named places —
  `crates/gamectl/src/scenario.rs` (`units_per_seat`, `MAX_UNITS_PER_SEAT`), `xtask/src/scenario.rs`
  (its twin), `crates/sim/src/world.rs` (`WorldConfig::units_per_seat` and the walkers), and
  `scenarios/README.md`'s key; every chain and golden its values move. **Branch**
  `feat/rules-s2-numbers`, worktree `../pharmakos-tune2`.
- **Builds:** the balance check extended to combat (Easy against Easy and each template against
  Easy, 20 seeds, 3 and 6 rounds; per seat per round kills, losses, value destroyed, the time to the
  first kill, and how each match ended), and a duel report holding the kit's counter relations (spec
  §9's table: autocannon against raiders, mortar out-ranging the autocannon, raiders diving a
  mortar, a mortar against a clump and a wall), each relation a line the owner reads, never a gate;
  every combat row proposed per decision 17 with its evidence; `g3r`'s power rows, repath cap, cell
  size and totals; S2-16: `units_per_seat` and its cap removed from the scenario format (a format
  change, every committed file has it absent), the walkers deleted from the sim, since G3′-real
  measured load without them; the rules text's note updated.
- **Implements:** item 144 (8); register S1-37, S2-08, S2-16, the S2 tuning rows. **Needs:** `opra`
  and `g3r` merged; decision 17. **Acceptance:** both reports attached; every chain and
  `report_hash` golden re-blessed once, the rows named; `mapgen`'s goldens re-blessed if
  `map_ceiling_kw` moves the generator; the main session's local package run. **Contract PR:** yes
  (the scenario format, `xtask`; `rules.proto` only if a row retires). **Agent-days:** 4–5.

### `demo2` — the run sheet, the certification and the register

- **Owns:** `docs/demo/s2-run-sheet.md` and the register's S2 section (the main session's docs PR).
  **Builds:** the run sheet for §1.1; G3′'s certification recorded; the nightly report with each
  band acknowledged as Easy's; the register's S2 rows closed or moved with their owners.
  **Implements:** AGENTS.md §10 items 5, 6 and 8. **Needs:** `tune2` and `arms` merged.
  **Agent-days:** 2–2.5.

---

## 4. The schedule

### 4.1 Waves

Three slots, never more. `crates/sim` has one author at a time; beside it run only the named places
`fall` (beside `site`) and `surv` (beside `dfnd`), each a new or separate module with a call line,
as `grid` ran beside `mine` in S1. Days are working days in plan units, five to a week; a task
starts the day its inputs merge, and a lane opens its PR when green, so the order inside a wave is a
merge order.

| Wave | Days | Slot 1 — sim | Slot 2 | Slot 3 | The owner can see |
|---|---|---|---|---|---|
| 0 | 0–2 | **msrv** (0.75) → **base** starts | → **redate** (0.75) → **rnds** (0.75) | → **east** (1.25) | 1.98 everywhere; `expand_east` reaches its site; `--rounds 6` |
| 1 | 0.75–7.25 | **base** (6.5) | **vfy1** (3.5, from 2.25) → **night** (5, from 5.75) | **con3** (3.5, from 2) → **con4** (3.5, from 5.5) | Live fielding; E8's verdicts agree with the compile |
| 2 | 7.25–17 | **fire** (9.75) | **night** → **wgw** (5.5, from 10.75) | **vfy2** (5.5, from 9) → **g3h** (4.5, from 14.5) | Shells, craters, wrecks, kill credit; the nightly harness forced once |
| 3 | 17–24.5 | **site** (7.5) | **view** (5.75, from 16.25) | **fall** (5.5, from 19, named place) | A wall going up, levelling, repair and salvage, a collapse |
| 4 | 24.5–31 | **dfnd** (6.5) | **surv** (3.5, named place) | — | Guards, pickets, the threat model |
| 5 | 31–40.25 | **atk** (9.25) | **oprd** (3, from 31) | **vfy3** (1.75, from 31) | A raid, a hunt, a breach, a charge |
| 6 | 40.25–49.25 | **tune2** (4.5, from 42.75) → **demo2** (2, from 47.25) | **opra** (2) → **arms** (2.5, from 42.25) | **g3r** (2.5) | The S2 demo of §1.1 |

Wave 4's third slot and wave 3's second after day 22 are idle by dependency, not by choice: nothing
off the sim's path is ready before `dfnd` merges. A lane that slips takes from that slack first.

**Totals.** 26 tasks in 25 lanes (`oprd` and `opra` are one lane in two PRs), **about 107 ad**
(95–119). **Critical path:** `msrv` → `base` → `fire` → `site` → `dfnd` → `atk` → `opra` → `g3r` →
`tune2` → `demo2`, **about 49 working days**, almost all of it the sim's serial lanes (about 39.5
days). Slot utilisation is about 72 % of 148 slot-days.

**Token cost.** At S1's 186–195 k sub-agent tokens per planned agent-day (item 115 (2)), 107 ad is
about **20–21 M** tokens; at the skeleton's wave-6 rate of 215–245 k, 23–26 M.

### 4.2 The re-bless ledger: how many times the chains move, and why

| PR | Scenario chains | Determinism chain | `report_hash` goldens |
|---|---|---|---|
| `east` | `expand-east-segment` and its event log | — | `expand_east`'s |
| `con3` (rows at today's constants) | — | — | all (`rules_hash`) |
| `base` (generational ids, beacons-lost counter) | all | **moved** | all (snapshot 10) |
| `vfy2` (`Scope`'s sightings) | — | — | all |
| `wgw` (the scope's content) | — | — | the gateway's |
| `fire` (combat columns; the core's cooldown in every match) | all, and its two new ones | **moved** | all (snapshot 11) |
| `site` | those with a Build, if drone state is hashed | — expected (Hold & Build's terraform reads NONE) | all if the snapshot bumps |
| `fall` | none expected (no committed run detaches a region) | — expected | — |
| `dfnd` | those reading `safest` or `SAFEST`, if a threat is in view; its new one | — expected: Defend's state is a table of its own with a row per Defend beacon, so a run with none hashes as before (item 136's block rule) | all (snapshot) |
| `surv` (the per-seat explored table) | all | **moved** | all (snapshot) |
| `atk` | its two new ones; any where Easy now attacks | — expected, as for `dfnd` | all (snapshot) |
| `opra` (Easy's sealed files) | `against-easy*` | — | the operator's and the `demo_*` |
| `tune2` (values; walkers removed; the power budget) | all | **moved** | all (`rules_hash`) |

**Over the stage the scenario chains move four times for certain** (`base`, `fire`, `surv`, `tune2`)
and up to four more (`site`, `dfnd`, `atk`, `opra`), **the determinism chain four times** (`base`,
`fire`, `surv`, `tune2`), and every `report_hash` golden eight times for certain (`con3`, `base`,
`vfy2`, `fire`, `dfnd`, `surv`, `atk`, `tune2`: the fixture snapshot is one of its inputs, so each
snapshot bump moves them, AGENTS.md §4.8) and a ninth if `site`'s drone state bumps the snapshot.
Folding `surv`'s hashed table into `base` would save one re-bless, but the table has no reader until
Survey's behaviour lands, and `base` would carry state for a lane two waves off; it is not
recommended. Each move is one behaviour change reviewable alone, and the numbers come last because
the check needs the finished rules.

### 4.3 Honest size

About 107 ad and a 49-day critical path in plan units, against the spec's 6 weeks (30 days); with a
week of float for contract reviews, about **10.8 plan-weeks**. The sim alone carries about 46 ad,
about 39.5 of it in series. S1's plan sized its row at about 7.1 plan-weeks against the spec's 5,
about 40 % over; S2's row is larger than S1's in every layer, and the §2.7 additions (collapse,
terraform, Defend's and Attack's full settings, the core's turret) are about 15 ad of it. The cut
order of section 4.5 takes about 3.5 days off the path; decision 1's second option takes about 7.5.

Plan units are not calendar days. S1's 71.5-ad plan ran from its adoption on 2026-09-29 to its close
on 2026-10-16. Its stops — the Windows CI leg, serial contract PRs, `MATRIX_ONLY`'s second matrix
for every lane touching `crates/sim/`, `crates/client-gdext/`, `godot/`, `crates/mesher/`,
`library/`, `rules/`, `.github/` or the chain goldens (`scripts/merge-train.sh` ~151) — set the
calendar again: about 20 of S2's 26 PRs pay it.

### 4.4 How lanes run in S2

- **`wave-lanes.js` is pointed at this plan** by `args.plan` = `docs/design/s2-plan.md` and
  `args.stage` = 'S2'; the task headings carry their ids in backticks ("### `base` —", "### `oprd` →
  `opra` —"), as S1's did. The environment's sentence on `cargo xtask determinism --bless` names the
  lanes whose section says the determinism chain moves (`base`, `fire`, `surv`, `tune2`).
- **The main session's docs PRs** carry, beside the rulings: the spec's worked example with `east`'s
  site (§10); spec §6's Build row (terraform's meaning, the base kit's blueprints, repair drones on
  work in hand), §6's Common row (decision 4's unset `roe`), §9 (the support rule, materials, the
  demolition charge's carrier), §17's S2 row (the cuts taken); AGENTS.md §3 rule 4's new client
  pieces (`rnds`' `--rounds` reader, `view`'s), §9 item 11 (G3′'s job), §10 item 5 (the bands read
  against Easy until S5); `docs/placeholders.md`'s S2 rows; `docs/design/README.md`'s row for this
  plan. AGENTS.md is a harness doc no lane edits.
- **Packaged locally before merging** (S1's decision 9): `con3`, `rnds`, `view`, `oprd`, `opra`,
  `tune2`, and any lane given a named place under `godot/`, `library/` or `rules/`.

### 4.5 The cut order, written before the stage starts (item 144 (7))

If S2 runs late, these move to S3, in this order, and nothing else does without the owner's word. A
cut construct is never silently ignored: the sim answers `NotAtThisStage("S3")` and the verifier its
E-code (E8), so a playbook that uses it is refused with a reason and the rules text says "from S3".

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
3. **Survey's refresh pass** (re-checking the sightings closest to leaving the reach window, enemy
   beacons first, before the next Lull). Scouts still scan unexplored ground first. It saves about
   1.5 ad, off the critical path (`surv` is a named place), so it buys review time rather than days.
   It goes last because it is part of a named row item ("full Survey"), and because S3, which builds
   the reach filters and the knowledge store's enemy-known predicates, is where a refresh pass has
   its readers.

**Checkpoints** (decision 32): cut 1 is taken if `fire` has not merged by day 19; cut 2 if `site`
has not merged by day 27; cut 3 if `dfnd` has not merged by day 34. Beyond the third cut, any
further cut is the owner's (section 7).

### 4.6 The contract pull requests, in merge order

Fifteen of S2's PRs touch an AGENTS.md §5 path. Each body names its paths; each new hashed field
goes into the state hash, the snapshot round-trip and the goldens in the same PR (§4.8); each
snapshot change bumps `SNAPSHOT_VERSION` (9 today, `crates/sim/src/snapshot.rs` ~153) and refuses an
older snapshot with a clear message. `SAVE_FORMAT_VERSION` (2, `crates/gateway/src/save.rs` ~99)
moves only if a lane changes the save container itself, which none is planned to; a save holding an
older snapshot is refused through the snapshot's own version. Event kinds and step failures are
added at the end with new names (E17).

| Order | PR | Contract paths |
|---|---|---|
| 1 | `msrv` | `clippy.toml`, the workspace manifest's `[workspace.package]` |
| 2 | `con3` | `proto/gp/v1/rules.proto` and the generated tree, comments in `playbook.proto` and `gateway.proto`, `GetEconomyForecastResponse` 5 and 6 `optional`, `ResolvedRef.reachable`, `rules/rules.v1.json` (`rules_hash`), golden contents |
| 3 | `base` | determinism code: ids and generations, the beacons-lost column, `SNAPSHOT_VERSION` 10; the event vocabulary (`StepFailure` 13) |
| 4 | `con4` | `playbook.proto`, `gateway.proto`, `proto/buf.yaml`'s scoped `ignore_only`, the generated tree, the schema and docs goldens |
| 5 | `night` | `xtask/src/scenario.rs` (what the `scenario` step checks), `.github/workflows/nightly-scenarios.yml` |
| 6 | `vfy2` | `report_hash`'s `Scope` encoding |
| 7 | `fire` | the combat phase, `Stream::Combat`'s first draw, the shell table, wall materials, the destroyed-value column, `SNAPSHOT_VERSION` 11, event kinds |
| 8 | `g3h` | `xtask/src/main.rs`' perf-alarm list; a sim seam for the walled harness |
| 9 | `site` | drone state and the snapshot if it adds any; the generated-surface golden (a new golden format); event kinds |
| 10 | `fall` | the Voxels phase's order; event kinds |
| 11 | `surv` | the explored table, the snapshot |
| 12 | `dfnd` | Defend's table, the snapshot; event kinds |
| 13 | `atk` | Attack's table, the snapshot; event kinds; `allocations.rs`' crowd |
| 14 | `g3r` | `cargo xtask g3-budget`, `.github/workflows/ci.yml`'s new job |
| 15 | `tune2` | the rules' values (`rules_hash`), the scenario format (the walkers' key removed), `xtask/src/scenario.rs` |

`NIGHTLY_SCENARIOS_ENABLED` is not a PR: it is a repository variable the owner sets (decision 30).

---

## 5. G3′-real, the depth task

**The criterion, verbatim from spec §16's G3′ row.** When: "Synthetic tick check in the spike; the
real measurement is S2's exit; scrub at S7". Go if: "4 seats (headroom over v1's 3), 300 units, 40
beacons: tick p99 ≤50% of the tick; ≥4× speed on the headless sim runner. The measurement sets the
per-map power budget for the generator. Scrub ≤1 s is judged at S7". Fallback: "Lower map power
budget; shorter segments". Item 144 (8) names it S2's depth task (AGENTS.md §10 item 6).

**What S2 certifies:**

1. **G3′-a, the tick's p99 ≤ 25 ms, and G3′-b, at least 4× real time on the headless runner (a mean
   ≤ 12.5 ms, ≥ 80 ticks a second),** at 300 units and 40 beacons as world totals (item 63) with
   decision 2's seats, over a full 8-minute segment of real play: combat, the support rule, the
   pathing the destruction causes (no charged substitute, the spike's 96.5 % caveat retired; CSR
   counted once, item 64), and a crater stream at the destruction rate the budget is set with.
2. **The per-phase figures the budget needs:** the tick-minus-pathing mean (item 116 (6) (b)); the
   repath backlog against the cap and the measured burst frequency, from which the cap is re-derived
   (item 69); the broadphase's share and its growth with density (item 67's caveat), which sets the
   cell size; the support search's worst tick at a region of `max_region_voxels` (item 138 (3));
   kill-credit settlement at the run's death rate (G3′ §9.12); live heap bytes over the segment
   (item 68); allocations per tick with a moving crowd (`atk`).
3. **The per-map power budget**, re-derived as item 65's arithmetic did, every input now a
   measurement, with `power.kw_per_unit`, `reserve_percent` and `map_ceiling_kw` moving together
   (item 91) and the destruction rate recorded in the same decision; the fielding totals (S1-37,
   S2-08) and the walkers (S2-16) with it.
4. **A recorded run** on the owner's demo machine (i7-9800X), the three CI runners' figures
   published beside it as notices, and E18's budget job green on `ubuntu-24.04`.

**The fallback, written now.** If G3′-a or G3′-b misses at the gate load, the spec's fallback is
taken in writing, with the owner's word: first a lower map power budget (fewer units the generator
can power), recorded as the new `map_ceiling_kw`; then, if that is not enough, shorter segments (a
ladder below 3 / 5 / 8, which interacts with the fun gate's own fallback, item 68). The repath cap
is not the fallback: it is item 69's lever and is re-derived in either case.

---

## 6. Owner decisions before the stage starts

Recommendation first, marked **(Recommended)**, downsides for each option. **[play]** marks the
play-shaping ones named one by one in the bulk question. Hygiene and engineering are not asked here;
they are section 2.3's calls.

**1. S2's size and schedule.** *Governs everything.*
- **(Recommended) Keep the whole row and let S2 run about 10.8 plan-weeks,** with section 4.5's cuts
  as the only planned reductions, taken at their checkpoints. *Downside:* later rows shift about
  five plan-weeks unless absorbed; the gate date is the owner's.
- *Two sim lanes: `site` beside `fire` as a named place* (Build's mandate and programs beside the
  combat phase), about 7.5 days off the path. *Downside:* two agents in the determinism crate at
  once, both bumping the snapshot and touching `tables.rs`, where a bad merge is a desync; S1's
  decision 1 refused the same trade.
- *Take all three cuts now:* about 3.5 days off. *Downside:* the stage ships less than the row, a
  moat lasts, and Survey is "full" in name only.

**2. [play] G3′'s seat count.** *Gates `g3h`.* The spec says "4 seats (headroom over v1's 3)"; v1
has at most three, the generator places three zones and the gateway refuses a fourth (E19).
- **(Recommended) Measure at three seats with the world totals (100 units a seat), and report a
  fourth, unplaced seat's decision-tick cost as a separate line.** Item 63's world totals already
  give each of three seats more load than four seats would (75 each). *Downside:* the spec's literal
  "4 seats" is not run; its headroom is argued, not measured.
- *Four placed seats on a four-zone test map:* *Downside:* a generator change for a configuration v1
  cannot play, and a gateway path no shipped build reaches.

**3. [play] Combat's place in the tick.** Section 2.3's E2 decides it as engineering (simultaneous
fire, every damage in the tick it is dealt); it is named here because it shapes what a player sees:
two assets that kill each other on one tick both die. **(Recommended)** as E2. *Downside:* no "first
shot wins". *Alternative:* fire resolves in seat order. *Downside:* the lowest seat wins every exact
duel.

**4. [play] An unset `roe`.** `playbook.proto`'s header makes unset an error and allows an
owner-ruled exception list of four omitted settings; nothing enforces it for `roe`, and Hold &
Build, the safe playbook and Easy's output all omit it.
- **(Recommended) Unset reads as `RETURN_FIRE`, a fifth entry on the list, with an I-level note.**
  The spec's Common row says return fire and retreat "follow the rules of engagement and are always
  announced", and its worked example writes `RETURN_FIRE`. The three values read as: `HOLD_FIRE`
  never fires; `RETURN_FIRE` fires only at an enemy asset that damaged one of the seat's own assets
  inside the firer's range within the under-attack window; `FREE_FIRE` fires at any enemy in range.
  The core's autocannon, which has no mandate of its own, fires as `FREE_FIRE` (decision 8). A unit
  below `retreat_hp_pct` walks to the nearest own beacon whose sphere holds a live repair drone,
  else to its home beacon, firing only as its `roe` allows (Defend's "fall back to the nearest
  friendly repair drone", spec §6, made common). No committed chain moves for this alone, since
  nothing is armed in a committed scenario before `fire`'s; the chains `fire` moves, it moves for
  its new columns. *Downside:* a fifth invented default, irreversible once files omit it.
- *Unset is an error (E-code):* *Downside:* every template, the safe playbook, Easy and the
  determinism playbook must write it, and every operator and `demo_*` golden moves.
- *Unset reads as `HOLD_FIRE`:* *Downside:* a seat that never wrote a `roe` never shoots back.

**5. [play] S2-11: what a seat sees of an enemy asset.**
- **(Recommended) In live vision now: the asset's kind, position, heading and exact HP; from memory:
  kind, position and the sighting's age, nothing else; a beacon's mandate, power state, units by
  role and build progress are never shown.** Attack's "weak point" and `weakest` read HP, and a
  shell or a wreck is plain to see. `get_beacon`'s prose follows (E9). *Downside:* HP is perfect
  information for what you see; perfect-intel snowballing is a spec §6 risk.
- *HP in bands (full, damaged, critical):* *Downside:* a second representation the selectors must
  rank, and a rule nobody stated.
- *Position only:* *Downside:* "weak point" and `weakest` cannot be computed for an enemy.

**6. [play] S2-02: the safe playbook's rescue rule.** Spec §14 says "flee and rescue" and states no
rescue.
- **(Recommended) Rescue is the commander's: after it takes damage, it walks to the nearest own
  beacon whose sphere holds a live repair drone and holds there until its HP is full or the segment
  ends; the flee rule is unchanged, and the safe playbook still never recycles, switches or
  places.** It uses what S2 adds (repair drones heal the commander, spec §4). Easy uses the same
  rule. Otherwise the safe playbook stays as spec §14 writes it under combat: its omitted `roe`
  reads per decision 4, and "the safest beacon" takes decision 12's threat term. *Downside:* an
  invented reading of one word.
- *Defer to S5 with Normal:* *Downside:* the safe playbook stays flee-only through the stage that
  makes it matter.

**7. [play] X-08 and the destroyed-value term.** The score is held value plus enemy value destroyed
(spec §3). `fixs` read the first tie-break, "unsmoothed net worth", as that score (`audit.rs`
~43–53), and the code's own note says the two readings agree only while destroyed value is zero and
rank score-tied seats in opposite directions once it is not.
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
a seat".
- **(Recommended) Enemy units only (the enemy commander included), nearest first.** *Downside:* an
  enemy mortar or wall built in its range is ignored; the raider is the mortar's counter (spec §9).
- *Units and structures:* *Downside:* the core out-trades a siege by itself.

**9. [play] Who carries a demolition charge.** Spec §9 names only "the carrier"; the commander
carrying one is roadmap (§4).
- **(Recommended) A raider of an Attack beacon with `breach_allowed`, loaded at `$` 15 when the
  breach needs it, planting at the breach point; a carrier killed before it plants drops a charge
  that never fires.** *Downside:* the raider's role widens; a charge cannot be placed by Build.
- *A build drone of the Attack beacon:* *Downside:* Attack beacons field drones only for this.
- *A new unit kind:* *Downside:* a sixth unit kind, which `rules/README.md` says does not exist, and
  its rows.

**10. [play] Walls and materials.** Spec: a wall is "cheap blocking terrain", priced per voxel, "HP
is by material" (`rules.proto` ~666).
- **(Recommended) A wall target is a box (`Area`) of voxels filled above the ground, each at
  `wall_cost_per_voxel`; a wall voxel has two levels (whole, cracked) and breaks on a blast at the
  second; any blast removes dirt, stone, ore and vent voxels inside its crater radius; the bottom
  layer is bedrock and the core's bore is never removed; wall material carries its seat (E14).**
  *Downside:* stone is no harder than dirt.
- *Per-material blast resistance for terrain too:* *Downside:* more rows and a tuning surface before
  the duel report exists.

**11. Collapse's constants, and an asset under a landing region.** Item 138 (3) asks for the
constants as proposed rows.
- **(Recommended) `collapse.max_region_voxels` = 4 096 and `collapse.fall_damage_hp` = 100 (both
  proposals, owner at S2's demo); an asset under the voxels a region lands on takes the same flat
  damage and stands on the new top.** *Downside:* a region of 4 097 voxels never falls, which a
  player may notice; 100 HP kills a scout and leaves a beacon standing, so item 144 (1)'s downside,
  "one crater under a beacon can kill it", holds at this value only for weaker assets, until the
  owner tunes it.
- *An asset under a landing region is destroyed:* *Downside:* a second damage rule, unbounded.

**12. [play] The threat model.** Spec §6 names threat-weighted targeting, `most_threatened`, "avoid
known threats" and SAFEST without a formula.
- **(Recommended) An enemy asset's threat is its weapon's damage per second (rows), zero if unarmed;
  `most_threatened` is the own beacon with the largest summed threat of enemy assets seen inside its
  sphere, ties to the lowest id; `beacon_under_attack` is the predicate's own window;
  threat-weighted targeting picks the highest threat inside the engagement radius, ties by distance
  then id; `AVOID_KNOWN_THREATS` adds the rules row's surcharge per step inside a known threat's
  range; `safest` subtracts the summed threat from its HP-fraction rank; `SAFEST` picks the nearest
  seam with no known threat within its range; `flee_on_threat` walks a mine drone home while a known
  threat is inside its beacon's sphere.** *Downside:* a model built from weapon rows only, blind to
  unarmed threats like a charge carrier until it is armed.

**13. [play] Defend's unstated settings.** Auto-fortify and unit mix have no stated meaning.
- **(Recommended) Defer both to S3's rule list, their numbers reserved, refused with E-codes; Defend
  fields raiders as guards and scouts as pickets on work in hand.** *Downside:* Defend reads thinner
  than the spec's row.
- *Define them now* (auto-fortify: the beacon queues an autocannon when a threat is seen; unit mix:
  the raiders' share of guards): *Downside:* two invented rules.

**14. [play] Attack's settings' values.** The spec names the settings, not their options.
- **(Recommended) Retarget: `NEAREST_KNOWN` or `NONE`; after-action: `REGROUP_AT_STAGING` or
  `RETURN_HOME`; retreat: below a percentage of the launch force, or after a game time; weak point:
  the target beacon's lowest-HP structure inside its sphere, else the beacon; waves: launches per
  segment (cut 1).** *Downside:* a vocabulary written now, irreversible once files use it.

**15. `BuildSettings` 6 and 7** (reserved as `repair_drones`, `reclaim_drones`). Spec §6:
"repair/reclaim drone fields", and "No fixed counts".
- **(Recommended) Both stay reserved; repair drones follow work in hand.** *Downside:* a player
  cannot turn salvage off.
- *Two counts:* *Downside:* contradicts "no fixed counts". *A job switch:* *Downside:* a sixth
  omitted-setting default.

**16. `report_kinds`** (`MandateSettings` 3). **(Recommended)** S3, with `get_reports`, which
answers `INTERNAL` today; S2's announcements ride the feed. *Downside:* a beacon cannot filter its
reports.

**17. [play] Combat's numbers, and how they are set.** The spec gives HP, costs, `kW` and speeds
(item 90) and no damage, range or rate. **(Recommended)** rows proposed from the kit's counter
relations, all `PLACEHOLDER: tuning — owner, at S2's demo`, checked by `tune2`'s duel and balance
reports, tuned at the demo:

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

*Downside:* the numbers are as good as the duel report and Easy's play.

**18. The alarm bands' wording.** **(Recommended)** "Rusher (Turtle, Hunter): an arm that is the
sole winner — last seat standing, or the final audit's — in more than 13 of the 20 seeds (65 %)
against Easy opens a tuning task. At S2 the band reads against Easy, which carries the Balanced
weighting (item 144 (6)); it is re-baselined against Normal at S5." *Downside:* a shared win counts
as no win, which favours not alarming.

**19 to 28. The register's owner rows not decided above.**
- **19. S2-03 and S2-04** are decision 12; **S2-01 and M-01** are `night`'s shape with decisions 18
  and 29 (Easy the one operator, item 144 (6)); **S2-07 and S2-09** are engineering calls E5 and
  E11, listed here for the owner's veto.
- **20. S2-05** becomes `locomotion.headroom_voxels` = 2 (`con3`), no behaviour change.
- **21. [play] The raider's target** (spec: "hits the nearest structure"). **(Recommended)** the
  nearest enemy structure or beacon by default; a raider guarding under Defend engages enemy units
  inside the engagement radius; under `RETURN_FIRE` it answers the unit that hit it. *Downside:*
  Defend widens the raider's stated role.
- **22. S1-34** becomes per-kind vision rows (decision 17's values).
- **23. S1-37** stands at 40 / 300 / 120 world totals, now counting live assets (E1), re-derived by
  `g3r`; **S2-16**: the walkers and their 256 cap are removed from the scenario format and the sim
  once G3′-real has measured load without them (`tune2`). *Downside:* a scenario can no longer add
  load by key; a load test drives a real match instead.
- **24. S2-06, `MAX_ROUTE_NODES` = 1 024,** stays a constant sized to the 384 map — a search bound,
  technically necessary — with an exhausted search a typed `no_path` counted under its own reason
  rather than a silent stop; re-derived with S4's map. *Downside:* a constant outside the rules
  table.
- **25. S2-08, the structure and wreck tables.** **(Recommended)** the wreck table's room equals the
  unit and structure rooms together, so every live asset can leave one; a wreck that finds it full
  replaces the oldest unsalvaged wreck, counted and announced (a proposed in-world reason: the ash
  buries old wrecks). *Downside:* a rule, and wrecks that expire.
- **26. S2-10, a dropped paid target.** **(Recommended)** removing a paid target from the list
  recycles what stands for it, refunding 50 % of its remaining value, as recycling does (spec §7).
  *Downside:* an edit can lose money. *Alternative:* it stays, finished never. *Downside:* a
  stranded asset.
- **27. S2-12, `get_view`'s delta mode.** **(Recommended)** measured by `wgw` and `view` at 300
  units and added as an opt-in request field only if the full list passes the client's per-frame
  budget; otherwise S7. *Downside:* the decision waits on a measurement.
- **28. S2-13** is decisions 13 and 14.

**29. [play] The nightlies' seat layout and "winning".** **(Recommended)** twenty seeds, each played
once, the arm alternating seats by the seed's position; the arm wins a seed when it is the sole
winner; `win_rate_percent` = wins × 100 / seeds, rounded down. *Downside:* the arm plays ten seeds
from each seat, not every seed from both. *Alternative:* both sides per seed (40 matches an arm).
*Downside:* "65 % of seeds" needs a new reading.

## 7. Owner decisions during the stage

**30. `NIGHTLY_SCENARIOS_ENABLED`.** A repository variable only the owner sets. **(Recommended)**
set when `arms` has merged and its forced run on `tune2`'s numbers is attached. *Downside:*
nightlies run on a stage-old Easy until S5.

**31. The G3′ budget job as a required check.** **(Recommended)** the owner adds `g3 budget
(ubuntu-24.04)` to the ruleset's required checks after it has passed on `main` five times running.
*Downside:* a shared runner's slow day reddens `main`; until then a regression shows only as a red
non-required check.

**32. Taking a cut.** **(Recommended)** the main session takes section 4.5's next cut at its
checkpoint and records it in the decisions log, under the delegation; any cut beyond the third is
put to the owner. *Downside:* a cut lands without a per-cut look.

**33. At S2's stage demo:** whether the merge delegation runs on into S3, and whether it covers the
radar lab's pull requests (items 140 (3) (a), 141 (1), (2)); the tuning of decision 17's numbers
from `tune2`'s reports; each band acknowledged as Easy's (AGENTS.md §10 item 5).

**34. If G3′ misses:** the fallback's order of section 5, with the owner's word.

A lane that finds one of these wrong in the code raises it rather than building around it.

---

## 8. Deliberately left out

**Out by AGENTS.md §11:** script runtimes, MCP, an SDK or published API, AI seats, manual control,
flags/branch/repeat, timed weapons and combat boosts (the charge's fuse is a fixed row, not a weapon
a player times), reinforced wall, gate and shield, beacon capture, flowing water, flying.

**Out because a later stage owns it.** **S3:** `report_kinds` and `get_reports` (decision 16); the
own-beacon and enemy-knowledge condition blocks (`playbook.proto` 43–49, 60–69) and E0209;
`list_known_enemies` and `query_area` (`INTERNAL` today); auto-fortify and unit mix (decision 13);
the cut items taken (section 4.5); FULL and the size budget, P1's counted units (item 139 (5)); the
editor findings of items 139 (2) and 143 (3), (4). **S4:** go-code launches, the Sensor Spire,
licences and their blueprints, the symmetric map, the map re-derivation that re-sizes
`MAX_ROUTE_NODES`. **S5:** Normal and Hard, target spread, the bands re-baselined. **S6:** art and
sound, HP bars beyond the plain ones `view` draws. **S7:** highlights and scrub. **After S2, beside
S3:** the radar lab (items 140, 141 (2)). **Only on evidence:** the commander's armour, decoys and
self-defence weapon, if Hunter's band shows hunting dominates (spec §4). **After v1:** physics
collapse (item 138 (3)), line of sight as a sensor model (item 144 (4)), tunnels in pathing (E23).

**Out with no owner this stage:** `xtask/src/package.rs`'s `smoke_env` (its next owner); G1's GPU
half on Linux, for want of a Linux GPU machine (section 9, R6).

**Out on principle:** a second hash function, a shadow proto, re-blessing a golden without naming
the rule that moved it, an invented rule where the spec is silent.

---

## 9. Risks, and the measurement that closes each

| # | Risk | What closes it | Where | If it does not close |
|---|---|---|---|---|
| R1 | The sim's serial path (39.5 days) slips, `fire` above all | Split seams; checkpoints; section 4.5's cuts | `fire`, `site`, `dfnd`, `atk` | Decision 1's second option, on the owner's word |
| R2 | A new hashed field (generations, cooldowns, shells, explored cells, mandate state) is missed | §4.8's three places per PR; save/restore round-trips in each sim PR; lens A | every sim lane | The cross-OS guard and resume tests catch it as a bisect |
| R3 | Repath bursts under craters break the p99 | The measured burst frequency; the cap re-derived | `g3h`, `g3r` | Section 5's fallback: a lower power budget |
| R4 | The support search's worst case | `max_region_voxels` measured inside G3′ | `fall`, `g3h` | A lower bound row, with its in-world reason |
| R5 | The broadphase grows super-linearly under combat queries | Its share measured; cell size from range (E11) | `g3h` | Item 67's rejected hierarchical grid, reconsidered on evidence |
| R6 | Re-meshing under a crater stream misses G1's three frames; G1's Linux GPU half never runs | `view`'s stream through the CPU proxy on three runners; an incremental relight | `view` | Coarser destruction (G1's fallback), the owner's |
| R7 | The nightlies measure Easy's weakness, not balance | Bands read against Easy (item 144 (6)) | `arms`, `demo2` | Re-baselined at S5 |
| R8 | Fifteen contract PRs queue on review | One proto and one `xtask` PR at a time; the week of float | section 4 | Decision 1's extension grows |
| R9 | The G3′ job is flaky on a shared runner | Spec numbers only on ubuntu; five passes before it is required (decision 31) | `g3r` | It stays a published notice |
| R10 | E1 changes what an id is, and every chain with it | One PR, first, re-blessed once with reasons | `base` | — |
| R11 | Breach on a 2.5D graph | Breach as a cost mode over wall columns (E23) | `atk` | Breach by mortar only; the charge waits |
| R12 | Friendly-fire avoidance leaves a mortar unable to fire near its own assets | The duel report's mortar lines | `tune2` | The owner re-rules avoidance at the demo |

---

*Sources: `docs/spec/pharmakos-spec-v0.6.html` §3–§9, §11, §14–§17, §19; decisions-log §2.7 items
13, 17, 40, 63–69, 90, 91, 104, 116, 123, 127, 128, 133–144; `docs/design/s1-plan.md`;
`docs/placeholders.md`'s S2 section; `docs/spikes/G3prime-segment-budget.md` §9;
`docs/spikes/G1-destruction-remesh.md`; AGENTS.md §3–§6, §9–§12; `.claude/workflows/wave-lanes.js`;
`scripts/merge-train.sh`; the code at `7a351e7`.*
