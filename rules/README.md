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
| `match.lull_ms` | — | **PLACEHOLDER** — Tuning, owner, at the skeleton's demo |
| `match.decision_tick_ms` 250 | item 90, spec §11 | **PLACEHOLDER** — tuning, owner, S3. Five sim ticks at 20 Hz |
| `locomotion.step_cost_*`, `climb_surcharge`, `move_cost_per_tick` | item 59 | 10 / 14 / 3 yes; `climb_surcharge = 4` has **no gameplay evidence behind it** (item 59) — owner, S3. `move_cost_per_tick` is **superseded** by the per-kind rows below (item 90) and nothing reads it; it stays because deleting a field is a `buf breaking` failure |
| `locomotion.commander_cost_per_second` 10 | item 90, spec §4 | **PLACEHOLDER** — tuning, owner, at the skeleton's demo. 1.0 voxels/s, the low end of the spec's range |
| `locomotion.drone_cost_per_second` 10 | item 90 | **PLACEHOLDER** — tuning, owner, S1 |
| `locomotion.raider_cost_per_second` 12 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `locomotion.scout_cost_per_second` 20 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `locomotion.repath_cap_per_tick` | item 69 | **PLACEHOLDER** — 16 is provisional, re-derived at S2's exit once the burst frequency is a measurement |
| `locomotion.fog_cost_*` 3 / 2 | item 61 | yes |
| `locomotion.hpa_cluster_voxels` 32 | item 58 | yes |
| `broadphase.cell_size_voxels` | item 67's caveat | **PLACEHOLDER** — Tuning, tied to unit density, owner at S2's exit. 16 is the cell edge spike G3′ actually measured with, and no more than that |
| `interface_times.*` | spec section 5's table | values are the spec's; all of section 5 is marked Tuning |
| `mesher.surfaces_per_frame` (K), `bytes_per_frame` (B) | item 54 | **PLACEHOLDER** — no measured frame-time reason separates K = 4 from K = 8 on the spike machine; owner, S6 art pass |
| `mesher.age_frames` | item 54 | **PLACEHOLDER** — ships untested by measurement and is labelled insurance |
| `mesher.light_max`, `light_atten` | — | **PLACEHOLDER** — Tuning, owner at S6's art polish |
| `economy.bmi_dollars` 100, `starting_bmi_multiplier` 2, `scaling_last_place_bonus_percent` 10, `scaling_leader_malus_percent` 5, `recycle_refund_percent` 50, `backlog_threshold_per_drone` 2, `ore_yield_per_voxel_dollars` 2/4/8, `seam_voxels` 150, `mining_ms_per_voxel` 2000 | item 90, spec §7 | **PLACEHOLDER** — tuning, owner, S1 |
| `economy.salvage_percent` 25 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `economy.award_fund_percent_of_bmi` 50 | item 90 | **PLACEHOLDER** — tuning, owner, S4 |
| `power.core_surplus_kw` 10, `generator_output_kw` 20/30/40, `revive_margin_kw` 2, `beacon_base_draw_kw` 2 | item 90, spec §7 | **PLACEHOLDER** — tuning, owner, S1 |
| `power.kw_per_unit` 1, `reserve_percent` 40, `map_ceiling_kw` 190 | item 91, inside item 65's budget | **PLACEHOLDER** — tuning, owner, at S2's exit. The three move **together**, and `map_ceiling_kw` is a *derived* number that is a row anyway, because two of its three inputs are measurements of the code rather than rules |
| `commander.cost_per_second` 10, `arrive_radius_voxels` 2, `placement_range_voxels` 12, `interface_range_voxels` 4 | item 90, items 11 and 21, spec §4 | **PLACEHOLDER** — tuning, owner, at the skeleton's demo |
| `commander.hp` 300, `respawn_base_ms` 30000, `respawn_growth_ms` 15000 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `beacon.sphere_radius_voxels` 24 | item 90, item 11, spec §5 | **PLACEHOLDER** — tuning, owner, at the skeleton's demo |
| `beacon.core_hp` 3000 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `map.size_*` 384/384/64, `spawn_zones` 3, `spawn_zone_radius_voxels` 24, `vent_*_distance_voxels` 28/44, `seam_*_distance_voxels` 8/20, `spawn_separation_pushes_*` 3/2 | item 90, spec §§3, 9, 15 | **PLACEHOLDER** — tuning, owner, at the skeleton's demo. The separation is a **fraction of one early Push of raider travel**, not a distance, because the distance that matters changes with the raider's speed and the segment length: one Push is 2 160 cost units, so 3/2 is 3 240, about 324 cardinal voxels. The vent band's upper end (44) is inside the reach of a beacon placed at the edge of the core's sphere (24 + 24 = 48): the placement range is commander-to-site, not core-to-site (item 95 corrects item 90's gloss) |
| `map.start_vent_richness` LEAN, `start_seam_richness` STANDARD, `contested_standard_vents` 2, `contested_rich_vents` 1, `contested_rich_seams` 3 | item 90 | **PLACEHOLDER** — tuning, owner, S1 |
| `units.build_drone`, `units.mining_drone` 20/100/1/10 | item 90, spec §§7, 9 | **PLACEHOLDER** — tuning, owner, S1 |
| `units.repair_drone` 25/100/1/10, `units.raider` 30/120/1/12, `units.scout` 10/60/1/20 | item 90 | **PLACEHOLDER** — tuning, owner, S2. `repair_drone` is item 90's *repair-reclaim drone* under its short name; there is no sixth unit kind |
| `units.starting_build_drones` 2, `units.starting_mining_drones` 1 | item 95, spec section 9 ("Tuning starting numbers") | **PLACEHOLDER** — tuning, owner, S1. The core and the commander are not counted: one of each per occupied seat by design |
| `structures.beacon` 60/800/2 | item 90, spec §5 | **PLACEHOLDER** — tuning, owner, at the skeleton's demo |
| `structures.generator` 80/600/0, `structures.survey_post` 30/200/1 | item 90 | **PLACEHOLDER** — tuning, owner, S1. The generator's `draw_kw` of 0 is omitted from the canonical JSON; see above |
| `structures.autocannon` 60/500/2, `structures.mortar` 90/400/3, `wall_cost_per_voxel` 1, `demolition_charge_cost_dollars` 15 | item 90 | **PLACEHOLDER** — tuning, owner, S2 |
| `structures.resonance_spire` 120/500/3 | item 90 | **PLACEHOLDER** — tuning, owner, S3 |
| `verifier.size_budget_units` 128 | item 94 | **PLACEHOLDER** — tuning, owner, at **S3's exit**; P1 sets the real number from the measured per-rule cost per decision tick. A unit is one route step, one handler, one step in a handler body, one build target, one protected area or one patrol waypoint; conditions and settings count nothing |
| `verifier.handler_cooldown_min_ms` 5000, `max_fires_max` 8, `notebook_max_chars` 4000 | item 90, spec §§10, 11 | **PLACEHOLDER** — tuning, owner, S3 |
| `verifier.reach_memory_ms` 180000 | item 90 | **PLACEHOLDER** — tuning, owner, S3 (item 90 marks it S2/S3) |

## Editing it

Write the JSON, then let the codec canonicalise it — field-number order, two
spaces, one entry per line — so the diff of a tuning change is one line:

```sh
cargo test -p pharmakos-proto            # writes target/golden/proto/actual.rules.v1.json
cargo xtask ci                           # the `golden` step compares it
```

`crates/proto`'s `the_rules_table_is_in_canonical_form` test fails if the file
is not already canonical, and names the first byte that differs.
