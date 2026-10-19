<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# The rules table

`rules.v1.json` is the canonical JSON of one `gp.v1.RulesTable`
(`proto/gp/v1/rules.proto`). It is decisions-log **item 78**: every tuning
value the sim, the verifier, the estimator and the mesher read lives in one
reviewable file, loaded by `pharmakos-sim` and hashed with item 48's encoder,
so `rules_hash` — one of the verifier's five inputs — covers all of it by
construction.

The split that makes this work:

- **The shape is a contract.** It is a `.proto` message, so `buf breaking`
  guards it in `WIRE_JSON` mode and a renamed row is caught the way a renamed
  playbook field is. Changing the shape needs owner approval (AGENTS.md §5).
- **The values are data.** A tuning change during S1 or S2 is an ordinary PR.
  It moves `rules_hash`, and therefore every golden hash chain and every
  `report_hash`, so the PR that moves it explains the movement (AGENTS.md
  §10 item 2) — but it is not a contract change.

This is what AGENTS.md §12 asks for in so many words: *tuning values are data,
versioned in the rules table and stamped into the rules hash — not constants
sprinkled through the code.*

## What `rules_hash` covers

**The whole file, not the rows the sim happens to read.** `pharmakos-sim`
decodes the file with the one canonical codec and hashes the canonical JSON the
codec re-emits, so every row is in the hash: the interface times the verifier
will read, the fog price and the cluster edge the estimator will read,
`match.lull_ms` that only the host reads, and `revision` and `note` as well.

That is a decision, and the alternative is the bug it avoids. If the hash
covered only the nine rows the tick reads today, a tuning pull request could
change `interface_times.place_beacon_deploy_ms` — changing what the verifier
reports — and leave `report_hash`, and every report cached under it, sitting
exactly where it was.

Two things follow:

- **Editing `note` moves `rules_hash`.** Intended: the hash identifies the
  table, and `revision` is supposed to move whenever a value does, so an edit
  that moves neither is an edit that changed nothing.
- **Reformatting does not.** The hash is over the codec's canonical form, not
  over the file's bytes, so whitespace cannot move it. `crates/proto`'s
  `the_rules_table_is_in_canonical_form` keeps the committed file in that form
  regardless.

## What is NOT in here

Anything that varies with the machine rather than with the game. G3′'s
calibration-constant lesson is that a performance knob inside hashed state is a
desync waiting for a slower laptop.

Two rows sit right on that line and are here deliberately:

- **the mesher's K and B.** A per-frame presentation budget — but item 54 makes
  the *drain order* a rule, so the whole block is data the mesher reads rather
  than constants it holds.
- **the per-tick repath cap.** Item 60 makes the cap part of the sim's
  behaviour: it decides which unit repaths on which tick.

Neither is a calibration constant, and the test is the same for both: the value
is the same number on every machine.

## Where the numbers come from, and which are still guesses

S1's `tune` lane (S1's plan, decision 16) ran the headless balance check
(`crates/gamectl/tests/balance.rs`, 80 two-seat matches against Easy) on this
table and moved **no value**. Decision 16 keeps a row without a spec range at
its item-90 seed unless the check flags it; the check flags a treasury that
only grows and a seat that stalls in almost every match, and both come from
what a seat can reach rather than from a price: after round 1 no uncovered
feature lies within one beacon's reach, so Easy seals its safe playbook and
nothing is left to buy (decisions-log item 136 (3) (q); S3's `toward` is the
stepping stone). No seat ever failed to afford a Generator. The pull request
carried the report, and S1's demo kept every economy row for S1 with it
(decisions-log item 139 (1) ruling 6): each is the owner's at S3 now, with the
balance check re-run once `toward` lets Easy reach past one beacon, as the
table below says row by row. The same lane changed only the table's `note`,
which states two grid rules no row holds (below), so `rules_hash` moved and no
chain did.

Revision 4 (S1's first contract pull request; decisions-log items 127 (2) and
128) sets the Lulls to the spec's lengths — `match.lull_ms` 180 000 → 300 000
and the new `match.first_lull_ms` 600 000 — and adds
`structures.build_hp_per_second` 60 and `economy.mining_carry_voxels` 16, the
sim's two named constants given rows at their own values, so no chain moves
until a lane changes one. S1's demo kept all four for S1 (item 139 (1) ruling
6): the two Lull rows are the owner's at S2's demo, and the two constants'
rows at S3, with the economy rows.

Revision 5 (S2's rules rows, S2's plan task `con3`; decisions-log item 144)
adds what S2's combat lanes read, so that no lane writes a number of its own:

- **rows that replace a constant, at the constant's value:**
  `locomotion.headroom_voxels` 2 (the sim's `HEADROOM_VOXELS`, register S2-05,
  E26) and `vision_radius_voxels` 8 on every unit kind, every structure kind
  and the commander (the scouts' `UNIT_VISION_RADIUS_VOXELS`, register S1-34).
  The vision markers carry decision 17's proposals for `tune2` to apply (E26
  keeps the headroom at 2; decision 17 proposes nothing for it), so no chain
  moves until a lane reads a row;
- **rows nothing reads yet, at the plan's proposals:** a `weapon` on
  `units.raider`, `structures.autocannon` and `structures.mortar` (whose
  `projectile` makes it indirect) and `beacon.core_weapon`; the `combat` block; the
  `defend` block's picket radius (spec §19); the `collapse` block (decision
  11); and the `materials` block, the terrain's blast levels (decision 10,
  ruled per material by item 144 (11)), the wall's minimum height (decision
  40) and the bedrock layers (E14).

Each is a PLACEHOLDER, the owner's at S2's demo with `tune2`'s reports. The
sim refuses a blast level outside 1 to 3 and zero bedrock layers at load, from
the lane that reads the row (`fire`); until then nothing reads them.

Revision 3 fills the table out: decisions-log **items 90, 91 and 94** fix every
value the walking skeleton's three lanes need, in one sitting, so that no task
writes a number of its own. Almost every row below is therefore a **labelled
guess** — sized so the plan's §1.1 demo is affordable (`$` 200 buys the beacon
and the Generator with `$` 60 over) and so the map's supply fits the ceiling
exactly (three seats: 3 × 10 + 3 × 20 + 2 × 30 + 40 = 190 kW). Item 90 records
the honest version: a number people plan against tends to stick, and the
rules-table home plus the loud label are the mitigation, not a proof.

**Eleven numbers are stated twice on purpose, and each pair must move
together.** A reader of the `units` or `structures` block should see a whole
unit or a whole building without cross-referencing two other blocks, so three
families are restated — and the `locomotion` and `power` side is the
authoritative one in each, because that is where the rule lives:

- **draw per fielded unit.** `power.kw_per_unit` (item 91) is restated as all
  five `units.*.draw_kw`.
- **a beacon's own draw.** `power.beacon_base_draw_kw` is restated as
  `structures.beacon.draw_kw` (both 2 kW).
- **walking speed.** `locomotion.drone_cost_per_second` is restated as
  `units.build_drone`, `units.mining_drone` and `units.repair_drone`'s
  `cost_per_second`; `locomotion.raider_cost_per_second` and
  `locomotion.scout_cost_per_second` likewise; and
  `commander.cost_per_second` repeats the same unit because the commander is
  not a unit kind.

**Two grid rules no row holds** are stated in the table's `note`, because
`rules.v1.json` is the rules text every client pins:

- **The commander walks through a blackout.** It keeps walking and draws
  nothing while its home beacon is dark: the one exception to dormancy parking
  a beacon's units, so a seat can always walk to a beacon and change something
  on site (decisions-log item 127 (5), register S1-35).
- **A priority raise re-applies the brownout order.** Raising a dark beacon's
  priority relights it at once when shedding lit beacons of lower rank covers
  it (draw no higher than supply after the swap), and the core outranks every
  priority, so a dark core may shed a lit HIGH expansion (item 127 (7) and
  item 135 (2) (a), register S1-22).

Nothing in the schema makes a pair move together, and the two halves are often
re-derived at different stages (item 90 moves the drone speed at S1 and the
raider's at S2; item 91 moves `kw_per_unit` at S2's exit). `crates/proto`'s
**`the_restated_rows_agree`** test is what catches a half-edit; if you add a
restated row, add it there too.

One row reads short in the file: `structures.generator.draw_kw` is **0** — the
Generator supplies rather than draws — and canonical proto3 JSON omits a
default-valued scalar, so the generator shows two numbers rather than three.
The decoded row is still 80 / 600 / 0. It is the table's first zero-valued row,
so it is the first place the omission is visible.

| Row | Source | Settled? |
|---|---|---|
| `match.segment_lengths_ms` 3 / 5 / 8 min | item 68 | yes |
| `match.lull_ms` 300 000 | item 127 (2), spec §3 | **PLACEHOLDER** — Tuning, owner, at S2's demo (item 139 (1) ruling 6: the two Lull rows stand for S1). Every Lull after the first: the spec's five minutes, re-ruling D-01's 180 000 for S1. Round 1 is timed by `first_lull_ms` and every later Lull by this row (S1's `econ`, #92) |
| `match.first_lull_ms` 600 000 | item 127 (2), spec §3 | **PLACEHOLDER** — Tuning, owner, at S2's demo (item 139 (1) ruling 6: the two Lull rows stand for S1). The first Lull: the spec's ten minutes (revision 4) |
| `match.decision_tick_ms` 250 | item 90, spec §11 | **PLACEHOLDER** — tuning, owner, S3. Five sim ticks at 20 Hz |
| `locomotion.step_cost_*`, `climb_surcharge`, `move_cost_per_tick` | item 59 | 10 / 14 / 3 yes; `climb_surcharge = 4` has **no gameplay evidence behind it** (item 59) — owner, S3. `move_cost_per_tick` is **superseded** by the per-kind rows below (item 90) and nothing reads it; it stays because deleting a field is a `buf breaking` failure |
| `locomotion.commander_cost_per_second` 10 | item 90, spec §4 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s balance report, whose spawn travel line reads this speed (item 139 (1) ruling 6: it stands for S1, after it stood at the skeleton's demo, item 126 (2) (b), and at S1's, with decision 16's `expand_east` column of `tune`'s balance report). 1.0 voxels/s, the low end of the spec's range. That column found the spec's `expand_east` cannot reach its first site in its 120 s at any speed inside the range: on the `expand-east-segment` scenario's seed the walk from either seat's core is 290 s or more at this speed, so it would need 2.5 voxels/s or faster, and item 139 (1) ruling 3 kept the speed: `expand-east-segment` gets its own playbook with a nearer first site and `examples/playbooks/expand_east.jsonc` is unchanged (item 103 (3); S2's plan, E22 as amended by the main session; decisions-log item 145, to come) |
| `locomotion.drone_cost_per_second` 10 | item 90 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). The three drone kinds' `cost_per_second` restate this row and each pair moves together: if `tune2` moves `units.repair_drone`'s speed in S2, this row and the other two drones' speeds move with it there (`crates/proto/tests/proto.rs`'s `the_restated_rows_agree`) |
| `locomotion.raider_cost_per_second` 12 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `locomotion.scout_cost_per_second` 20 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `locomotion.repath_cap_per_tick` | item 69 | **PLACEHOLDER** — 16 is provisional, re-derived at S2's exit once the burst frequency is a measurement |
| `locomotion.fog_cost_*` 3 / 2 | item 61 | yes |
| `locomotion.hpa_cluster_voxels` 32 | item 58 | yes |
| `broadphase.cell_size_voxels` | item 67's caveat | **PLACEHOLDER** — Tuning, tied to unit density, owner at S2's exit. 16 is the cell edge spike G3′ actually measured with, and no more than that |
| `interface_times.*` | spec section 5's table | ratified, decision 15 (S1's plan; item 128; register U-04): the spec's section 5 times are the rows, each changed element of a list counts as one field, and a deploy pays no separate handshake |
| `mesher.surfaces_per_frame` (K), `bytes_per_frame` (B) | item 54 | **PLACEHOLDER** — no measured frame-time reason separates K = 4 from K = 8 on the spike machine; owner, S6 art pass |
| `mesher.age_frames` | item 54 | **PLACEHOLDER** — ships untested by measurement and is labelled insurance; owner, S6 art pass |
| `mesher.light_max`, `light_atten` | — | **PLACEHOLDER** — Tuning, owner at S6's art polish |
| `economy.bmi_dollars` 100, `starting_bmi_multiplier` 2, `scaling_last_place_bonus_percent` 10, `scaling_leader_malus_percent` 5, `recycle_refund_percent` 50, `backlog_threshold_per_drone` 2, `ore_yield_per_voxel_dollars` 2/4/8, `seam_voxels` 150, `mining_ms_per_voxel` 2000 | item 90, spec §7 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). The scaling percentages are the spec's +10 % and −5 %, inside its range |
| `economy.mining_carry_voxels` 16 | S1-25, S1's plan decisions 10 and 13 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at revision 4's value, the sim's named constant it replaced, because no flag of the check implicates it (S1's plan, decision 16). Ore voxels a mining drone carries home, read by the sim at load (`RulesTable::mining_carry_voxels`, S1's `mine`, #86). Named `carry` rather than the register's `load`, which the gateway's schema guard refuses (`rules.proto` says why) |
| `economy.salvage_percent` 25 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `economy.award_fund_percent_of_bmi` 50 | item 90 | **PLACEHOLDER** — tuning, owner, S4 |
| `power.core_surplus_kw` 10, `generator_output_kw` 20/30/40, `revive_margin_kw` 2, `beacon_base_draw_kw` 2 | item 90, spec §7 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon, unless S2's exit lowers `power.map_ceiling_kw` below the map's supply (mapgen's supply-fits-ceiling rule, item 91), when they move with it under `g3r`/`tune2` (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). The two grid rules above ride with this block. `beacon_base_draw_kw` is restated as `structures.beacon.draw_kw`, and the restated half moves with it, in S2 when this row moves there (`crates/proto/tests/proto.rs`'s `the_restated_rows_agree`) |
| `power.kw_per_unit` 1, `reserve_percent` 40, `map_ceiling_kw` 190 | item 91, inside item 65's budget | **PLACEHOLDER** — tuning, owner, at S2's exit. The three move **together**, and `map_ceiling_kw` is a *derived* number that is a row anyway, because two of its three inputs are measurements of the code rather than rules |
| `commander.cost_per_second` 10, `arrive_radius_voxels` 2, `placement_range_voxels` 12, `interface_range_voxels` 4 | item 90, items 11 and 21, spec §4 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s balance report (item 139 (1) ruling 6: each stands for S1, after each stood at the skeleton's demo, item 126 (2) (b), and at S1's). `cost_per_second` is the commander's speed the sim, the verifier's walking bound and decision 16's `expand_east` column read, and that column found the spec's `expand_east` cannot reach its first site in its 120 s at any speed inside spec section 4's range (see `locomotion.commander_cost_per_second` above, which restates this row and must move with it: `crates/proto/tests/proto.rs` holds the two equal). `placement_range_voxels` is **superseded by walk-in** (item 127 (12), amending item 11): `place_beacon` walks to its site, and since targeting's behaviour lane (`tgt`, #81) nothing reads the row (the deploy needs only the interface range, the sim's `site_is_legal`); it stays only because deleting a field is a `buf breaking` failure |
| `commander.hp` 300, `respawn_base_ms` 30000, `respawn_growth_ms` 15000 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `beacon.sphere_radius_voxels` 24 | item 90, item 11, spec §5 | **PLACEHOLDER** — tuning, owner, at S2's demo (item 139 (1) ruling 6: it stands for S1, after it stood at the skeleton's demo, item 126 (2) (b), and at S1's) |
| `beacon.core_hp` 3000 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `map.size_*` 384/384/64, `spawn_zones` 3, `spawn_zone_radius_voxels` 24, `vent_*_distance_voxels` 28/44, `seam_*_distance_voxels` 8/20, `spawn_separation_pushes_*` 3/2 | item 90, spec §§3, 9, 15 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s balance report, whose spawn travel line reads the spawn pair (item 139 (1) ruling 6: it stands for S1, after it stood at the skeleton's demo, item 126 (2) (b), and at S1's). The separation is a **fraction of one early Push of raider travel**, not a distance, because the distance that matters changes with the raider's speed and the segment length: one Push is 2 160 cost units, so 3/2 is 3 240, about 324 cardinal voxels. The vent band's upper end (44) is inside the reach of a beacon placed at the edge of the core's sphere (24 + 24 = 48): the placement range is commander-to-site, not core-to-site (item 95 corrects item 90's gloss) |
| `map.start_vent_richness` LEAN, `start_seam_richness` STANDARD, `contested_standard_vents` 2, `contested_rich_vents` 1, `contested_rich_seams` 3 | item 90 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon, unless S2's exit lowers `power.map_ceiling_kw` below the map's supply (mapgen's supply-fits-ceiling rule, item 91), when they move with it under `g3r`/`tune2` (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16) |
| `units.build_drone`, `units.mining_drone` 20/100/1/10 | item 90, spec §§7, 9 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). Each restated half moves with its pair in S2 when the pair's other half moves there: the `cost_per_second` restates `locomotion.drone_cost_per_second`, which moves in S2 if `tune2` moves `units.repair_drone`'s speed, and the `draw_kw` restates `power.kw_per_unit`, which moves at S2's exit under `g3r`/`tune2` (`crates/proto/tests/proto.rs`'s `the_restated_rows_agree`) |
| `units.repair_drone` 25/100/1/10, `units.raider` 30/120/1/12, `units.scout` 10/60/1/20 | item 90 | **PLACEHOLDER** — tuning, owner, S2. `repair_drone` is item 90's *repair-reclaim drone* under its short name; there is no sixth unit kind |
| `units.starting_build_drones` 2, `units.starting_mining_drones` 1 | item 95, spec section 9 ("Tuning starting numbers") | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). The core and the commander are not counted: one of each per occupied seat by design |
| `structures.beacon` 60/800/2 | item 90, spec §5 | **PLACEHOLDER** — tuning, owner, at S2's demo for the cost and HP only, with `tune2`'s balance report (item 139 (1) ruling 6: it stands for S1, after it stood at the skeleton's demo, item 126 (2) (b), and at S1's). `draw_kw` restates `power.beacon_base_draw_kw` and moves with it (`crates/proto/tests/proto.rs`'s `the_restated_rows_agree`). `draw_kw` is the gross draw: a placed beacon adds **no net draw**, because its own key-core supplies its base draw (item 113 (5)) |
| `structures.generator` 80/600/0, `structures.survey_post` 30/200/1 | item 90 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at item 90's seed, because no flag of the check implicates it (S1's plan, decision 16). The generator's `draw_kw` of 0 is omitted from the canonical JSON; see above |
| `structures.autocannon` 60/500/2, `structures.mortar` 90/400/3, `wall_cost_per_voxel` 1, `demolition_charge_cost_dollars` 15 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `structures.build_hp_per_second` 60 | S1-24, item 105 (1), S1's plan decisions 10 and 13 | **PLACEHOLDER** — tuning, owner, at S3, with the balance check re-run once `toward` lets Easy reach past one beacon (item 139 (1) ruling 6: it stands for S1), after `tune`'s balance check report kept it at revision 4's value, the sim's named constant it replaced, because no flag of the check implicates it (S1's plan, decision 16). Hit points a build drone adds per second, read by the sim at load (`RulesTable::build_hp_per_tick`, S1's `build`, #97). The table loads only when it is a positive multiple of the tick rate, so it moves in steps of 20 (item 136 (3) (b)) |
| `structures.resonance_spire` 120/500/3 | item 90 | **PLACEHOLDER** — tuning, owner, S3 |
| `verifier.size_budget_units` 128 | item 94 | **PLACEHOLDER** — tuning, owner, at **S3's exit**; P1 sets the real number from the measured per-rule cost per decision tick. A unit is one route step, one handler, one step in a handler body, one build target, one protected area or one patrol waypoint; conditions and settings count nothing |
| `verifier.handler_cooldown_min_ms` 5000, `max_fires_max` 8, `notebook_max_chars` 4000 | item 90, spec §§10, 11 | **PLACEHOLDER** — tuning, owner, S3 |
| `verifier.reach_memory_ms` 180000 | item 90 | **PLACEHOLDER** — tuning, owner, S3 (item 90 marks it S2/S3) |
| `locomotion.headroom_voxels` 2 | S2-05, S2's plan E26 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s report. The sim's `HEADROOM_VOXELS` given a row at its own value (revision 5); S2's `ids` swaps the constant for the row. E26 keeps today's 2; decision 17 proposes nothing for it |
| `units.*.vision_radius_voxels`, `structures.*.vision_radius_voxels`, `commander.vision_radius_voxels` 8 | S1-34, item 144 (4), S2's plan decisions 17 and 22 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s report. Every kind holds the scouts' constant (`UNIT_VISION_RADIUS_VOXELS`), so the sim's answer is unchanged until S2's `ids` reads the rows. Decision 17 proposes scout 24, raider 12, commander 10, the drones 8, autocannon 20, mortar 12, Generator and survey post 8; none for a beacon, which sees by its sphere, or the resonance spire |
| `units.raider.weapon` 4 / 8 / 500, `structures.autocannon.weapon` 20 / 12 / 250, `beacon.core_weapon` 24 / 12 / 250 (range voxels, HP, cooldown ms) | S2's plan decision 17 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s duel report. Direct fire: no `projectile`, so the damage lands in the tick it is dealt (E2). Read from S2's `fire` |
| `structures.mortar.weapon` 40 / 60 / 4000, projectile 8 voxels/s, splash 3, crater 2 | S2's plan decision 17 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s duel report. The `projectile` (the shell) is what makes it indirect (E13); the splash has no falloff. Read from S2's `fire` |
| `combat.under_attack_window_ms` 10000, `threat_step_cost_multiplier` 4, `charge_radius_voxels` 4, `charge_fuse_ms` 5000, `repair_hp_per_second` 20, `terraform_ms_per_voxel` 1000 | S2's plan decision 17 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s reports. Nothing reads them until the S2 lane that builds each rule |
| `defend.picket_radius_voxels` 6 | spec §19, S2's plan decision 17 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s report. Beyond the sphere's edge; read from S2's `dfnd` |
| `collapse.max_region_voxels` 4096, `fall_damage_hp` 100, `max_visits_per_tick` 16384 | item 138 (3), S2's plan decision 11 | **PLACEHOLDER** — tuning, owner, at S2's demo. Read from S2's `fall` |
| `materials.blast_levels` dirt 1, stone 2, ore 1, vent 1 | item 144 (11), S2's plan decision 10 | **PLACEHOLDER** — tuning, owner, at S2's demo, with `tune2`'s duel report. Each from 1 to the catalogue's 3 (E14); the ore and vent grades share their kind's row; a wall's two levels are the ruling's, not a row. Read from S2's `fire` |
| `materials.wall_min_height_voxels` 2, `bedrock_layers_voxels` 1 | S2's plan decision 40, E14 | **PLACEHOLDER** — tuning, owner, at S2's demo. Bedrock is never removed, and 0 layers is refused at load, because the support rule needs a floor. Read from S2's lanes: bedrock from `fire`, the wall's height from `site` and `vfy2` |

## Editing it

Write the JSON, then let the codec canonicalise it — field-number order, two
spaces, one entry per line — so the diff of a tuning change is one line:

```sh
cargo test -p pharmakos-proto            # writes target/golden/proto/actual.rules.v1.json
cargo xtask ci                           # the `golden` step compares it
```

`crates/proto`'s `the_rules_table_is_in_canonical_form` test fails if the file
is not already canonical, and names the first byte that differs.
