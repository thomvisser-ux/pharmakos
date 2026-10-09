<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The PLACEHOLDER register: the walking skeleton

Every value the walking skeleton guessed, one row per distinct decision, grouped by when it is
resolved and then by who resolves it (AGENTS.md section 12; decisions-log item 123 (2) 13). It is
hand-curated: the grep below finds the lines, and a person read each one to decide what it guesses,
who resolves it and when. Nothing in the code was reworded to make this register; rewording an entry
is the owning lane's work, and on a contract path its own pull request's (AGENTS.md section 5).

Anchors are a file plus a symbol or a heading, never a line number. A row lists every copy of its
decision. The row IDs are this file's own and exist only so that the cross-check at the end can name
a row.

## How the numbers were made

- **Base:** `main` at `640d5b90e423127253706ca0cdfc2cad6a07c411` (T21b's last commit, merged
  2026-09-28 at 04:27 UTC).
- **Command:** `git grep -n -I PLACEHOLDER -- ':!docs'`, run from the repository root.
- **Raw count:** 588 lines in 116 files. For reference, the same command over the whole tree prints
  752 lines, and 753 without `-I` (the extra line is a binary match in
  `crates/proto/src/generated/descriptor.binpb`). At `a80fb72`, where `main` stood when this
  register was written (item 123's docs pull request on top of the base, which changed
  `docs/design/` alone), the command prints the same 588.

From 588 lines to 256 rows, one step at a time:

| Step | Lines | Left |
|---|---|---|
| Raw count | | 588 |
| Generated lines under `crates/proto/src/generated` (`gp/v1/gp.v1.rs` 91, `gp/api/v1/gp.api.v1.rs` 11): 100 copies of a `proto/**` comment, and the `HAS_PLACEHOLDERS` enum's two code lines in `gp.api.v1.rs` (`as_str_name` and `from_str_name`) | −102 | 486 |
| The enum value `HAS_PLACEHOLDERS`, not a guess (`gateway.proto`'s `Applicability`, `crates/verifier/src/structure.rs`'s `on_death_stub` doc, and three verifier report goldens) | −5 | 481 |
| Rule text: `AGENTS.md` 3, `CLAUDE.md` 3, `.claude/workflows/wave-lanes.js` 4 | −10 | 471 |
| References and tombstones: 27 lines that mention another entry or the word without making a guess of their own, and 11 that say a PLACEHOLDER was discharged | −38 | 433 |
| Continuations: a second PLACEHOLDER word inside the same comment block as the first | −16 | 417 |
| Pointers (`PLACEHOLDER, as above.`, `see [X]`, `the module docs' PLACEHOLDER`) folded into the entry they point at | −23 | 394 |
| Tuning lines delegated to `rules/README.md`'s "Settled?" table: `proto/gp/v1/rules.proto` 75, `rules/README.md` 32, and 11 restatements (`crates/sim/src/rules.rs` 7, `crates/verifier/src/limits.rs` 2, `crates/mesher/src/bake.rs` 1, `crates/mesher/src/upload.rs` 1) | −118 | 276 |
| Copies merged: a marked entry that repeats a decision another entry already carries, listed in that row's anchors | −61 | **215** |
| The 19 `rules.proto` rows due "at the walking skeleton's demo", listed one by one in "At this demo" | +19 | 234 |
| One delegation line per stage for the other tuning rows (S1, S2, S3, S4, S6) | +5 | 239 |
| Carry-forwards that carry no marker: item 123 (4) 8 (S1-03 to S1-10), item 123 (5) 2 (S1-44, S1-45) | +10 | 249 |
| Unmarked guesses added by hand (the "Unmarked" section) | +7 | **256** |

S1-49 and S1-50 were added by item 131, after this count; item 133 added S1-51 and S3-19 and moved S1-27 to S1-30 to S4; item 135 added S2-14, S3-20 and S3-21 and moved S1-46 to S3; item 136 added S1-52, S2-15, S3-22 and S6-40, and moved S1-17 to S1-19 to S5, S1-40 to S6 and S5-04 to S3.

Appendix B lists every line of the 38, 16 and 23 above, and every line of the 10 and 5, by file and
symbol, so each step can be reproduced. The 61 merged copies are the extra anchors in the rows.

**Rules for placing a row.** A row sits in exactly one section. Its section is the earliest stage its
entry names; an entry naming two stages ("S2/S5", "S3/S6") sits in the first and says so. An entry
whose only "when" is a closed task sits in "Stale", and one with no "when" at all sits in
"Malformed", each with the stage and the who this register proposes, marked **proposed**. An entry
that names a "when" but no "who" sits in its stage under "No one named". An entry whose "when" fits
no section goes to the nearest one with a note saying so.

---

## At this demo

The owner rules on these at the stage demo's review (decisions-log item 123 (3)). **Ruled on
2026-09-29 (item 126 (2) (b)): every row stands as it is for now, and no rules row is re-tuned, so
S1-09 carries nothing.** D-01 is re-ruled for S1 by item 127 (2): the spec's 10-minute first Lull
and 5-minute later ones, with a first-Lull row, in S1's first contract pull request. The comments of D-20 to D-27 are reworded by the next lane that owns each
file. A tuning value the owner changes is a `rules.proto` and `rules/rules.v1.json` change that
moves the rules hash and every chain, so it goes to S1's first contract pull request (item 123 (4));
the carry-forward S1-09 would have booked that.

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| D-01 | `match.lull_ms` = 180 000: every Lull, the first included, lasts three minutes; the spec's default is five, with ten for the first Lull, and Probation's Lulls are untimed. Whether the lobby's Lull length becomes a match setting the host knows is S1's with the Probation preset (T16a notes, section D) | `proto/gp/v1/rules.proto` `Match.lull_ms`; `rules/README.md` Settled? row `match.lull_ms`; unmarked copy: `godot/scripts/mesher_rules.gd` `RULES_JSON`, read by `crates/client-gdext/src/bridge.rs` | item 123 (2) 4 |
| D-02 | `locomotion.commander_cost_per_second` = 10 (1.0 voxel a second, the low end of the spec's range) | `rules.proto` `Locomotion.commander_cost_per_second`; `rules/README.md` row `locomotion.commander_cost_per_second` | item 90 |
| D-03 | `commander.cost_per_second` = 10, which restates D-02 (`crates/proto`'s `the_restated_rows_agree` holds them equal); the sim reads this one | `rules.proto` `Commander.cost_per_second`; `rules/README.md` row `commander.cost_per_second …`; `crates/sim/src/rules.rs` `RulesTable` field `commander_cost_per_second` | item 90 |
| D-04 | `commander.arrive_radius_voxels` = 2 | `rules.proto` `Commander.arrive_radius_voxels`; `rules/README.md` row `commander.cost_per_second …` | item 90 |
| D-05 | `commander.placement_range_voxels` = 12 | `rules.proto` `Commander.placement_range_voxels`; same README row | items 90, 11, 21 |
| D-06 | `commander.interface_range_voxels` = 4 | `rules.proto` `Commander.interface_range_voxels`; same README row | items 90, 11 |
| D-07 | `beacon.sphere_radius_voxels` = 24 | `rules.proto` `Beacon.sphere_radius_voxels`; `rules/README.md` row `beacon.sphere_radius_voxels` | items 90, 11 |
| D-08 | `map.size_x` = 384 | `rules.proto` `Map.size_x`; `rules/README.md` row `map.size_*`; `crates/sim/src/rules.rs` `RulesTable` field `map_size_voxels` | item 90 |
| D-09 | `map.size_y` = 384 | `rules.proto` `Map.size_y`; same two copies as D-08 | item 90 |
| D-10 | `map.size_z` = 64 | `rules.proto` `Map.size_z`; same two copies as D-08 | item 90 |
| D-11 | `map.spawn_zones` = 3 | `rules.proto` `Map.spawn_zones`; `rules/README.md` row `map.size_*` | item 90 |
| D-12 | `map.spawn_zone_radius_voxels` = 24 | `rules.proto` `Map.spawn_zone_radius_voxels`; same README row | item 90 |
| D-13 | `map.vent_min_distance_voxels` = 28 | `rules.proto` `Map.vent_min_distance_voxels`; same README row | items 90, 95 |
| D-14 | `map.vent_max_distance_voxels` = 44 | `rules.proto` `Map.vent_max_distance_voxels`; same README row | items 90, 95 |
| D-15 | `map.seam_min_distance_voxels` = 8 | `rules.proto` `Map.seam_min_distance_voxels`; same README row | item 90 |
| D-16 | `map.seam_max_distance_voxels` = 20 | `rules.proto` `Map.seam_max_distance_voxels`; same README row | item 90 |
| D-17 | `map.spawn_separation_pushes_numerator` = 3 | `rules.proto` `Map.spawn_separation_pushes_numerator`; same README row | item 90 |
| D-18 | `map.spawn_separation_pushes_denominator` = 2 | `rules.proto` `Map.spawn_separation_pushes_denominator`; same README row | item 90 |
| D-19 | `structures.beacon` = 60 `$` / 800 HP / 2 kW (its comment's net-draw sentence is carry-forward S1-04) | `rules.proto` `Structures.beacon`; `rules/README.md` row `structures.beacon` | item 90 |
| D-20 | The default round limit, 6 (spec section 3), which "the owner ratifies with the lobby at T22". The lobby names its own limit (`godot/scripts/host_link.gd` `ROUND_LIMIT`, 6 at the base, 3 with T22a; item 123 (2) 3; S1-40) and so never reaches this default; `gamectl scenario run`, `gamectl seat doctor` and the sim's determinism binary do. Once ruled, the comment is the next sim lane's to reword (item 123 (4)) | `crates/sim/src/runner.rs` `DEFAULT_ROUND_LIMIT` | items 100 (7), 123 (2) 3 |
| D-21 | The camera's speeds, easing, zoom limits and starting pose (walled floats) | `godot/scripts/camera_rig.gd` `PAN_RATE`, and by pointer `ZOOM_STEP`, `TURN_RATE`, `FOLLOW_RATE`, `NEAREST`, `yaw`, `pitch`, `distance` | plan T16 |
| D-22 | The camera's field of view and far plane | `godot/scenes/vista.tscn` node `Camera` | plan T16 |
| D-23 | `PICK_REACH` = 4096, a pick ray's reach, "with the camera's other numbers at the demo review" | `crates/client-gdext/src/view.rs` `PICK_REACH` | — |
| D-24 | The watch speeds 1×, 2×, 4× ("2–4×" does not say whether 3× is a speed); the comment dates it to "T16's review with the demo" | `crates/client-gdext/src/pacer.rs` `SPEEDS` | T16a notes, section D |
| D-25 | **Now:** whether scouts, and Survey-lite's sightings, join the live view (spheres only today; the sightings are S3's) | `crates/gateway/src/host.rs` `SphereVision`; `crates/sim/src/sight.rs` module doc | item 108 (1); w6 notes D question 4 |
| D-26 | **Now:** that an in-process seat has a rate limit of its own at all, taken on the recommendation. Its numbers, 128 calls a tick and 1 200 a window, are the owner's at hardening with the rate limits (and H-06) | `crates/gateway/src/limit.rs` `IN_PROCESS_LIMITS` | item 111 (4); w6 notes D question 1 |
| D-27 | **Now:** where a resume lands (a Push save resumes into the same Push; a Lull save into its Lull), taken on the recommendation | `crates/gateway/src/surface.rs` `Surface::resume` doc | w6 notes D question 2, decision C8 |

Also ruled on at the review, and not PLACEHOLDERs (item 123 (3), with item 124 (4)'s correction):
the skeleton plan's PROPOSED mark (item 87), lifted; the section 1.1 deviations, which the run sheet
lists (`docs/demo/skeleton-run-sheet.md`), accepted; the deferral of P2's baseline to S5 and P6's
to S3's playtest (item 123 (2) 11), accepted; and the delegation of items 85, 86, 116 (2) and (9),
and 120, which the skeleton's end returned to the owner, renewed for S1 (item 126 (2)).

---

## S1

### The owner, contract carry-forwards (item 123 (4))

Each goes to S1's first contract pull request (AGENTS.md section 5).

| ID | What is guessed or stale | Anchors | Source |
|---|---|---|---|
| S1-01 | The toolchain pins overtaken by events: the channel is still `stable` although 1.98.1 was decided when G4 produced its golden hash, and the MSRV is still "1.85" in two places | `rust-toolchain.toml` `[toolchain] channel`; `Cargo.toml` `[workspace.package] rust-version`; `clippy.toml` `msrv` | item 123 (4); `docs/spikes/G4-determinism.md` |
| S1-02 | `.cargo/config.toml` still waits on "the G4 determinism spike" to decide pinned codegen flags, and G4 is closed | `.cargo/config.toml` header comment above `[net]` | item 123 (4) |
| S1-03 | `gateway.proto`'s `headroom_kw_now` comment, false after the first settle (no marker) | `proto/gp/api/v1/gateway.proto` `GetEconomyForecastResponse.headroom_kw_now` | item 113 (4) |
| S1-04 | A `structures.beacon` sentence saying a placed beacon adds no net draw (no marker) | `proto/gp/v1/rules.proto` `Structures.beacon` | item 113 (5) |
| S1-05 | `xtask`'s "Required means required" module doc lists missing workspace metadata as a failure, while three consumers fall back; making them fail changes what `ci` does | `xtask/src/main.rs` module doc "Required means required"; `step_golden`; `walled_packages`; `research_feature_spec` | item 123 (4) |
| S1-06 | `PHARMAKOS_REQUIRE_TOOLS=0` still means on: the variable is read as set or unset | `xtask/src/main.rs` `REQUIRE_TOOLS_VAR` and its readers; `crates/proto/tests/generated.rs` module doc and its require-tools reader | item 123 (4) |
| S1-07 | The children that bypass `child_command`, so `--require-tools` does not reach them: `perf-alarms`, `load_workspace`'s `cargo metadata`, the tool probes and `xtask/src/package.rs`'s children | `xtask/src/main.rs` `child_command`, `perf_alarms`, `load_workspace`; `xtask/src/package.rs` | item 123 (4) |
| S1-08 | The premise of `REUSE.toml`'s pointer-block comment ("so each listed pointer resolves here"): `reuse` ignores files named `LICENSE*` | `REUSE.toml` block "Licence pointers and directory placeholders" | items 123 (1), 123 (4) |
| S1-09 | The rules rows due at this demo that the owner re-tunes at the review (D-01 to D-19): a change re-blesses every chain. **Ruled 2026-09-29: none re-tuned (item 126 (2) (b)); nothing to carry.** | see "At this demo" | item 123 (4) |
| S1-10 | A strict one-line PLACEHOLDER grammar and an `xtask` check that collects this register | this file; `xtask` | items 123 (2) 13, 123 (4) |

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S1-11 | What `_status.phase_remaining_ms` shows outside a Push: an untimed Lull and a recap both read 0, like a Lull whose timer ran out; whether they become a distinguishable "no countdown", with the recap screen **Discharged by `econ` #92 (item 135): the footer carries `con2`'s `Status.untimed` for the lobby, a recap, an ended match and a Lull no host has reported a countdown for; a Push is never untimed, and a clock report with no `remaining_ms` is no countdown, not a countdown at 0. The marker in `sync_time` is gone.** **`ui` #98 (item 136) builds the client half: the lobby shows no countdown while the footer says `untimed`, and `ClockPhase::Lull` holds `Option<u64>`, so untimed and a timer at 0 cannot be confused.** | `crates/gateway/src/surface.rs` `sync_time` doc ("No countdown is told apart from one that ran out", the ruling; the marker is gone) | T16a notes, section D |
| S1-12 | `get_economy_forecast` answers the live world during a Push although its name says "forecast"; spec section 3 allows a seat its own score live and does not name `$` or kW **Discharged by `econ` #92 per decision 12 (item 135): the live answer in a Push stays, and the gateway's marker is gone; `gateway.proto`'s comment already records the ruling.** | `crates/gateway/src/surface/knowledge.rs` `get_economy_forecast`; `proto/gp/api/v1/gateway.proto` `GetEconomyForecastResponse` | w6 notes D |
| S1-13 | A seat's core is its lowest-numbered beacon; a column the day a beacon carries a kind. **Discharged in the gateway by `tgtw` #84 (item 133): `core_beacon_of` is the seat's `b_00` (per-seat ordinal 0); the `verifier_scope` doc's stale copy is gone, the doc now S3-04's alone. `gateway.proto`'s `BeaconSummary.core` marker still says "lowest-numbered"; the row closes when the next `proto/**` pull request rewords it.** | `crates/gateway/src/surface/knowledge.rs` `core_beacon_of`; `gateway.proto` `BeaconSummary.core` | — |
| S1-14 | A placed beacon's added draw is read as zero in every goal that places one **Discharged by `oper` #96 (item 136): `PLACED_BEACON_ADDED_DRAW_KW` is deleted, a placed beacon drawing net zero by construction (S1-31), and `a_placed_beacon_is_charged_no_draw_however_tight_the_grid` pins it.** | `crates/operator/src/candidates.rs` `PLACED_BEACON_ADDED_DRAW_KW` | items 90, 113 (5) |
| S1-15 | A Generator goal always places a new Build beacon rather than adding to an existing beacon's Build list **Discharged by `oper` #96 (item 136): a live vent the seat covers inside an own lit Build beacon's sphere is a tap, one `interface` step adding a Generator `on {feature_id}`, checked with at most two calls (`TAP_PROBE_CALLS`); the marker is gone.** | `crates/operator/src/candidates.rs` `enumerate` | item 113 (6) |
| S1-16 | "Inside one of the seat's own non-core spheres" read as "mined already" **Discharged by `oper` #96 per decision 14 (item 136): read through the gateway's `covered`, so a seam the seat covers, the starting seam under the core included, offers no Mine goal; `mine_site` and its marker are deleted.** | `crates/operator/src/candidates.rs` `mine_site` | — |
| S1-20 | `SPHERE_MARGIN_VOXELS` = 2, until placement legality is on the wire as an estimate. **Its need is discharged by `tgtw` #84 (item 133): `estimate_route` accepts `covering` and returns the site the sim's `cover` chooses. Deleting `SPHERE_MARGIN_VOXELS` is `oper`'s.** **Discharged by `oper` #96 (item 136): `SPHERE_MARGIN_VOXELS` is deleted with `site_for`; Easy asks `estimate_route` with `covering`.** | `crates/operator/src/easy.rs` `SPHERE_MARGIN_VOXELS` | — |
| S1-21 | The safe playbook's reading of "power is short" and "at risk" (spec section 14 defines neither) **Discharged by `oper` #96 per decision 14 (item 136): short is the draw above the supply (an own beacon dark, or `draw_kw_now > supply_kw_now`), and at risk is lit, not the core, not HIGH and next in the brownout's own order; `safe.rs`'s module doc states it and its marker is gone.** | `crates/operator/src/safe.rs` module doc, and by pointer `power_short` and `at_risk` | item 113 (4); w6 notes D |
| S1-22 | Whether a priority raise re-applies the brownout order, so a raised dark beacon relights and a lit lower-priority one sheds **Discharged in the sim by `grid` #91 (item 135): `power.rs`'s `swap_in`, inside `settle`, relights a dark beacon when shedding lit beacons of lower rank covers it, the core outranking every knob; the `program_for` marker is gone. `oper` #96 (item 136) rewords the copy in `crates/operator/src/safe.rs`'s module doc to the swap, the core outranking every knob, and the row closes.** | `crates/sim/src/programs.rs` `program_for` doc (first of three); copy in `crates/operator/src/safe.rs` module doc | item 113 (4); plan T14b |
| S1-23 | What "one field" means for the interface's 0.5 s step when a setting is a list. **Discharged by `proj` #85 per decision 15 (item 133): each element of a list is one field of the edit, still capped at 6 s; the function is now `crates/verifier/src/interface.rs` `count`, re-exported by plan-core, whose open half is S1-51.** | `crates/verifier/src/interface.rs` `count` (moved from plan-core by `proj` #85) | — |
| S1-24 | `BUILD_HP_PER_SECOND` = 60, proposed as `structures.build_hp_per_second` **Discharged by `build` #97 (item 136): the sim reads `structures.build_hp_per_second` at load (`RulesTable::build_hp_per_tick`), refused unless a positive multiple of the tick rate, and `BUILD_HP_PER_SECOND` is deleted. `rules/README.md`'s row still says the sim reads a constant: `tune` rewords it; the row's own tuning PLACEHOLDER (owner, at S1's demo) stands. `crates/proto/tests/proto.rs` (~1027) still asserts the row at 60 with the message "S1-24: the sim's BUILD_HP_PER_SECOND, as a row at its own value": the next `crates/proto` change rewords it.** | `crates/sim/src/economy.rs` `BUILD_HP_PER_SECOND`; `crates/proto/tests/proto.rs` (~1027) | item 105 (1) |
| S1-25 | `MINING_LOAD_VOXELS` = 16, now the row `economy.mining_carry_voxels` (`con1`; the name is item 129 (2) (a)'s). **`mine` #86 (item 133) reads the row through `RulesTable::mining_carry_voxels` and deletes `MINING_LOAD_VOXELS` and its marker; the row's own tuning PLACEHOLDER in `rules.proto` (owner, at S1's demo) stands, and `rules/README.md`'s line for it is reworded by the next lane that touches `rules/`.** | `proto/gp/v1/rules.proto` `mining_carry_voxels` marker ("tuning — owner, at S1's demo"); `crates/sim/src/economy.rs` `MINING_LOAD_VOXELS` deleted by `mine` #86 | — |
| S1-26 | The determinism harness: its segment list (20 000 / 15 000 ms), its harness playbook, its 1 200-tick run and its fifty walkers per seat, all deleted when `DETERMINISM_TICKS` is raised to a real segment. Two copies still name T14 | `crates/sim/src/lib.rs` `DETERMINISM_SEGMENT_LENGTHS_MS`, `determinism_playbook`; `xtask/src/main.rs` `DETERMINISM_TICKS`; `crates/sim/tests/determinism.rs` `GOLDEN_TICKS`; `tests/golden/determinism/README.md` "What the committed chain covers (T10)"; `crates/sim/src/world.rs` `fill_unit_table`; stale copies naming T14: `WorldConfig::units_per_seat`, `draw_new_destinations`; unmarked copy: `crates/sim/src/lib.rs` `DETERMINISM_MATCH_SEED` | item 116 (6)(e) |
| S1-31 | The key-core is netted out of draw rather than shown as an output in supply **Discharged by `grid` #91 per decision 12 (item 135): `PowerRules` reads `power.beacon_base_draw_kw`, a beacon's key-core supplies exactly that base, so a beacon nets to zero by construction; the module-doc marker is reworded to the ruling. The meter's wording is `ui`'s, and `ui` #98 (item 136) writes it (`godot/scripts/strings.gd` `meter_key_core`): supply and draw leave out each beacon's base draw, which its key-core supplies. The row closes.** | `crates/sim/src/power.rs` module doc, and by pointer `draw_of` | item 113 (5); plan T14b |
| S1-32 | Two vents are one when their patches share a span; a vent table would make it true by construction. **Discharged by `tgt` #81 (item 131): `one_vent` reads the feature table.** | `crates/sim/src/power.rs` `one_vent` | — |
| S1-33 | The brownout order sheds a beacon whose shed relieves nothing, ahead of the core **Discharged by `grid` #91 (item 135): `brown_out` skips a beacon whose shed relieves nothing or deepens the deficit, and it stays lit; the test is inverted to `a_beacon_whose_shed_relieves_nothing_is_skipped_and_stays_lit`.** | `crates/sim/src/power.rs` `brown_out`, and by pointer `crates/sim/tests/economy.rs` `a_beacon_whose_shed_relieves_nothing_is_skipped_and_stays_lit` (renamed by `grid` #91 from `…_is_still_shed_ahead_of_the_core`) | item 113 (5); plan T14b |
| S1-34 | A unit's sight radius, `UNIT_VISION_RADIUS_VOXELS` = 8, and the view's "recompute everything on a change" key. The copies disagree on the stage: `programs.rs` says S2, `watch.rs` and `host.rs` "S1 at the latest", `viewfeed.rs` "S1/S3" | `crates/sim/src/programs.rs` `UNIT_VISION_RADIUS_VOXELS`; `crates/gateway/src/surface/watch.rs` `sight_key`; `crates/gateway/src/viewfeed.rs` `Sight`; `crates/gateway/src/host.rs` `SphereVision` (its second clause) | item 105 (1); T16a notes, section D |
| S1-35 | The commander walks through a brownout, drawing nothing while its home is dark ("argued, not decided") **Discharged in the sim by `grid` #91 (item 135): the `program_for` doc states the ruling and `the_commander_walks_through_a_blackout_drawing_nothing` pins it. The rules-text sentence of item 127 (5) rides with `tune`'s rules pull request.** | `crates/sim/src/programs.rs` `program_for` doc (second) | item 10 |
| S1-36 | A unit's program is chosen by its kind, never by its home beacon's writ **Discharged by `grid` #91 (item 135): the `program_for` doc states the ruling, its marker gone.** | `crates/sim/src/programs.rs` `program_for` doc (third) | item 90 |
| S1-37 | The fielding limits: `BEACON_TABLE_ROOM` = 40 and `UNIT_TABLE_ROOM` = 300, per match, shared per seat since `tgt` (the total ÷ the seat count, rounded down, item 127 (12)), as is `STRUCTURE_TABLE_ROOM` (S2-08's total); the per-seat numbers are the owner's at S1's demo with the balance check's report (item 131), and the totals are revisited at S2's G3′-real measurement | `crates/sim/src/world.rs` `BEACON_TABLE_ROOM`, `UNIT_TABLE_ROOM`, `STRUCTURE_TABLE_ROOM`, `room_per_seat` | item 63 |
| S1-38 | `MineSettings`' `dig_max_depth`, `pillar_spacing`, `seam_choice` and "never digs under structures" are not applied; the skeleton digs the nearest ore and one safe layer. **Discharged by `mine` #86 (item 133): `crates/sim/src/mining.rs` applies all four settings and the held seam; the two markers went with `dig_stand`.** | `crates/sim/src/world.rs` `ore_in_sphere` (its marker gone); `dig_stand` deleted by `mine` #86 | — |
| S1-39 | How an omitted enum or setting inside the body reads (`MoveStep.pace`, `MineSettings.pillar_spacing` and `seam_choice`): one decision for all three. **Discharged by `tgtv` #80 per decision 11 (item 131): each, and `BuildSettings.terraform`, reads as a named default with an I0003 note where a beacon's settings start from nothing (a `place_beacon`'s `initial`, a `set_mandate` switch), never on a `set_mandate_settings` edit.** | `crates/verifier/src/structure.rs` `walk_action` doc; `proto/gp/v1/playbook.proto` `MineSettings` | — |
| S1-41 | Values the scenario format cannot set: `units_per_seat` when S1's units arrive, `round_limit` when a scenario plays a match to its end. **Nearest section:** the entry says "the stage that first needs it" | `crates/gamectl/src/scenario/run.rs` module doc | item 102 (7) |
| S1-49 | `BENCH_SEED`, the map P1's per-unit figure is certified on (today the committed scenarios' seed); owner, at S1's demo P1 run | `crates/bench/src/decision.rs` `BENCH_SEED` | item 131 |
| S1-50 | `DETERMINISM_SITE_OFFSET_VOXELS` = 3, how far east of its core the harness deploy stands since no stacking; owner, at S1, the `tune` lane, with S1-26's real determinism segment | `crates/sim/src/lib.rs` `DETERMINISM_SITE_OFFSET_VOXELS` | item 131 |
| S1-52 | `CORE_DIG_MAX_DEPTH` = 4, the core's dig depth Easy and the safe playbook write in their `deepen_core` step, held equal to Expand & Mine's own `dig_max_depth`; a review's probe found the starting seam dry at depth 4 by round 5 on both seeds. Owner, at S1's demo, with the economy's numbers (`tune`) | `crates/operator/src/easy.rs` `CORE_DIG_MAX_DEPTH` | items 133 (3) (a), 136 |

### A named lane

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S1-42 | Fold `plan_fingerprint` into the sim as `hash::plan_fingerprint` "the next time `crates/sim` is open (S1 at the latest)"; the value must not move. The proto copy is stale: it says nothing computes a fingerprint, and the verifier and the gateway do **Discharged in the sim and the verifier by `build` #97 (item 136): `pharmakos_sim::hash::plan_fingerprint`, its value unmoved (`the_plan_fingerprint_is_unmoved`), and the verifier's marker gone. `crates/proto/src/fingerprint.rs`' module doc still carries the marker and says the verifier computes the fingerprint; the row closes when the next `proto/**` pull request rewords it.** | `crates/verifier/src/hash.rs` `report_hash` doc; stale copy `crates/proto/src/fingerprint.rs` module doc (names T2) | item 77 |
| S1-43 | The still-commented `[workspace.dependencies]` versions, checked against the registry by the lane that uncomments each. **Nearest section:** no stage is named | `Cargo.toml` `[workspace.dependencies]` header comment | — |
| S1-44 | `no_arithmetic.rs`'s scanner matches operators only, so arithmetic methods pass it; widening it flags `rig.rs`'s keep-alive sum and needs AGENTS.md rule 4's wording on the keyframe back-off (an owner decision). The next `client-gdext` lane. **Nearest section:** no stage is named **Discharged by `ui` #98 per decision 17 (item 136): the scanner flags arithmetic methods, a method path handed on uncalled and Duration's conversions; `rig.rs`'s keep-alive sum is named word for word in `SCHEDULING_SUMS`, and AGENTS.md section 3 rule 4 names it.** | `crates/client-gdext/tests/no_arithmetic.rs`; `crates/client-gdext/src/rig.rs` keep-alive | item 123 (5) |
| S1-45 | `tests/rate_budget.rs` and `tests/phase_budget.rs` each carry their own stand-in gateway; one could serve both. The next lane that touches either. **Nearest section:** no stage is named | `crates/client-gdext/tests/rate_budget.rs`, `phase_budget.rs` | item 123 (5) |
| S1-51 | The sim's interface pricing brought to decision 15: the sim's `mandate_fields` still counts a list, and a Build target list, as one field of the edit, so the Push charges a multi-element list or targets less than the editor quotes and W0701 can fire on a route the Push fits. The owner of `crates/sim` in S1's wave 4, `build`, which moves those scenarios' chains **Discharged in the sim by `build` #97 (item 136): a Build target is its own row at `build_target_ms`, each element of a list one field, `mandate_fields` the verifier's `count` field for field, and the sim's pointer is gone. `crates/verifier/src/interface.rs`' module doc and `count`'s doc still carry the old reading and the marker; the row closes when S2's first verifier lane rewords them.** | `crates/verifier/src/interface.rs` `count` doc | item 133 |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S1-47 | Ordered units and Build targets enter the verifier's projection with the economy and the real Quartermaster, where `E0601`, `W0601` and `W0602` get their emitters. **Discharged by `proj` #85 (item 133): the estimate stage emits them over `crates/verifier/src/projection.rs`, which plan-core re-exports.** | `crates/plan-core/src/projection.rs` module doc | item 82 |
| S1-48 | `get_map_summary`'s vents and seams (`con2`'s `features`, for `tgtw`), unfogged in S1; the fog filter (item 130 (3) (a)) and the terrain summary (item 130 (3) (b)) are S3. **Done by `tgtw` #84 (item 133): `get_map_summary.features`, unfogged in S1; the fog filter stays S3's.** | `gateway.proto` `GetMapSummaryResponse` | — |

### Tuning, delegated

- `rules/README.md`'s Settled? table, the rows it marks S1: 7 rows (25 `rules.proto` fields:
  `locomotion.drone_cost_per_second`; `economy.*` except `salvage_percent` and
  `award_fund_percent_of_bmi`; `power.core_surplus_kw`, `generator_output_kw`, `revive_margin_kw`,
  `beacon_base_draw_kw`; `map.start_*_richness` and `contested_*`; `units.build_drone`,
  `mining_drone`, `starting_build_drones`, `starting_mining_drones`; `structures.generator`,
  `survey_post`).

---

## S2

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S2-01 | The nightly `gamectl scenario run` flags (the seed set, the Balanced operator's selection, a machine-readable result) do not exist | `.github/workflows/nightly-scenarios.yml` comment above the adversarial run step | item 116 (6)(j) |
| S2-02 | The safe playbook has the flee rule only; spec section 14's rescue rule waits for combat. **Names S2/S5** | `crates/gateway/src/host.rs` `SAFE_PLAYBOOK`; `crates/operator/src/safe.rs` module doc; `library/safe_playbook.jsonc` header | w6 notes D |
| S2-03 | `AVOID_KNOWN_THREATS` routes exactly as `DIRECT` | `crates/sim/src/interpreter.rs` field `avoid_known_threats` | — |
| S2-04 | The `safest` selector ranks by HP fraction; the threat term waits for combat | `crates/sim/src/interpreter/cond.rs` module doc | — |
| S2-05 | `HEADROOM_VOXELS` = 2 has no rules row | `crates/sim/src/pathing.rs` `HEADROOM_VOXELS` | — |
| S2-06 | `MAX_ROUTE_NODES` = 1024, sized against the 384-voxel map | `crates/sim/src/pathing.rs` `MAX_ROUTE_NODES` | — |
| S2-07 | The tick's queues: `HARNESS_EDIT_QUEUE` = 64 and `HARNESS_DAMAGE_QUEUE` = 256 | `crates/sim/src/world.rs` `HARNESS_EDIT_QUEUE`, `HARNESS_DAMAGE_QUEUE` | item 65 |
| S2-08 | `STRUCTURE_TABLE_ROOM` = 120 and `WRECK_TABLE_ROOM` = 120 (the totals; `STRUCTURE_TABLE_ROOM`'s per-seat split is S1-37) | `crates/sim/src/world.rs` `STRUCTURE_TABLE_ROOM`, `WRECK_TABLE_ROOM` | — |
| S2-09 | `BROADPHASE_QUERY_RADIUS_VOXELS` = 16, until a weapon's range replaces it | `crates/sim/src/world.rs` `BROADPHASE_QUERY_RADIUS_VOXELS` | item 67 |
| S2-10 | A Build target dropped from the list strands what was already paid for there | `crates/sim/src/world.rs` `replace_build_targets` doc | item 23 |
| S2-11 | What a seat may see of an enemy beacon or asset beyond where it is (HP, power state, heading, build progress, mandate) | `gateway.proto` `ViewEntity` (numbers 6 to 15); `gateway.proto` `BeaconSummary` (numbers 7 to 15) | T16a notes, section D |
| S2-12 | An opt-in delta mode for `get_view`'s entity list, once 300 units make a full list 20–30 KB a poll | `gateway.proto` `GetViewResponse.entities` | — |

### A named lane

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S2-14 | A verifier E-code for a seal whose `$`/kW projection leaves the sim's types or the forecast's `int32`, so the editor shows before submit what `submit_plan`'s `seal_commitment` door refuses (until then FULL gives only W0602 or W0603 there). The next owner of `crates/verifier`, which no remaining S1 lane is: S2's first verifier lane | `crates/gateway/src/surface.rs` `Surface::seal_commitment` | item 135 (2) (d) |
| S2-15 | A verifier E-code for a `scout_count` past 255, which the sim refuses at compile (`PlanError::ScoutsOutOfRange`) and `submit_plan` therefore refuses with `INVALID_ARGUMENT`, so the editor shows it before submit. S2's first verifier lane, beside S2-14 | `crates/sim/src/interpreter.rs` `compile_settings_row` (`PlanError::ScoutsOutOfRange`) | item 136 (5) |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S2-13 | `DefendSettings` and `AttackSettings` are stubs, numbers 1 to 15 held | `proto/gp/v1/playbook.proto` `DefendSettings`, `AttackSettings` | plan T1 |

### Tuning, delegated

- `rules/README.md`'s Settled? table, the rows it marks S2 and S2's exit: 10 rows (19 `rules.proto`
  fields: 14 at S2 — `locomotion.raider_cost_per_second` and `scout_cost_per_second`, `economy.salvage_percent`,
  `commander.hp` and the respawn pair, `beacon.core_hp`, `units.repair_drone`, `raider`, `scout`,
  `structures.autocannon`, `mortar`, `wall_cost_per_voxel`, `demolition_charge_cost_dollars` — and 5
  at S2's exit — `locomotion.repath_cap_per_tick`, `broadphase.cell_size_voxels`,
  `power.kw_per_unit`, `reserve_percent`, `map_ceiling_kw`). The README's `repath_cap_per_tick` row
  names no owner, which its `rules.proto` source and `crates/sim/src/rules.rs` copy do.

---

## S3

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S3-01 | The map menu's Visit & change offers the priority row only; mandate and Build-target rows come with the pickers | `crates/client-gdext/src/editor.rs` `Action::Visit` | — |
| S3-02 | The placement ghost is per click, not live on hover. **Names S3/S6** | `crates/client-gdext/src/editor.rs` `GhostState`, and by pointer `preview_place`; `godot/scripts/editor.gd` header | w6 notes A4, A6 |
| S3-03 | `gamectl verify` keeps its reference view, so a beacon outside its spheres gets E0403 although the game accepts it; revisited with Save/Load | `crates/gamectl/src/seat.rs` module doc | item 117 (1) |
| S3-04 | A seat's sighted enemy beacons join the verifier's scope with the knowledge store | `crates/gateway/src/surface.rs` `verifier_scope` doc (reworded to this row alone by `tgtw` #84; its old `is_core` clause was S1-13) | — |
| S3-05 | Real terrain fog (never seen, last known, the ×1.5 bound) as one mask feeding the view and the route estimator, which is when legs are dashed | `crates/gateway/src/viewfeed.rs` module doc | item 107 (1); T16a notes, section D |
| S3-06 | The interpreter's own ceilings: `MAX_ROUTE_STEPS` = 256 and `MAX_HANDLERS` = 64 | `crates/sim/src/interpreter.rs` `MAX_ROUTE_STEPS`, `MAX_HANDLERS` | item 94 |
| S3-07 | A step's tags are counted, not stored; a selector's tag filter is refused | `crates/sim/src/interpreter.rs` field `tags` | — |
| S3-08 | An absent `on_fail` means `SKIP` | `crates/sim/src/interpreter.rs` `DEFAULT_ON_FAIL` | — |
| S3-09 | `max_deaths_before_fallback` = 0 means no limit | `crates/sim/src/interpreter/exec.rs` `out_of_deaths` | — |
| S3-10 | Spike G2's section 9.9 values as rules rows: `LONG_RUN` = 6, `SMOOTH_LOOKAHEAD` = 16 | `crates/sim/src/pathing.rs` `LONG_RUN`, `SMOOTH_LOOKAHEAD` | G2 section 9.9 |
| S3-11 | The corner entrance per cluster, recorded and deliberately not built | `crates/sim/src/pathing/clusters.rs` module doc | plan section 2.1 |
| S3-12 | `TARGETS_PER_BEACON` = 8, settled with the size budget at S3's exit | `crates/sim/src/world.rs` `TARGETS_PER_BEACON` | item 94 |
| S3-13 | `SIGHTINGS_PER_SEAT` = 64 | `crates/sim/src/world.rs` `SIGHTINGS_PER_SEAT` | — |
| S3-14 | No probe area is ever recorded as visited, so a Survey mandate works its first area only | `crates/sim/src/world.rs` `probe_to_enter`, and by pointer the Survey program's `run` in `crates/sim/src/programs.rs` | — |
| S3-15 | The condition depth and node limits need rules rows if they become tuning | `crates/verifier/src/limits.rs` module doc | — |
| S3-16 | The segment clock and the "fits" pill are not built. **Names S3/S6** | `godot/scripts/editor.gd` header | plan T19 |
| S3-17 | Chip editing and drag reordering in the rule list | `godot/scripts/rule_list.gd` header | item 111, decision C3 |
| S3-18 | Travel times are shown as raw game milliseconds until the gateway answers a rendered figure (items 57 and 61) | `godot/scripts/strings.gd` `leg`; `godot/scripts/editor.gd` header | items 57, 61 |
| S3-19 | The verifier's travel is a lower bound (`walking.rs`), so W0701 warns only on a route that cannot fit (the sim has priced interface rows as the verifier does since `build` #97, item 136, so S1-51's gap is closed); pathfinder-quality legs per known leg, priced by the gateway and handed in with `Scope`, come with the segment clock and the fits pill (S3-16) | `crates/verifier/src/walking.rs` module doc | item 133 |
| S1-46 | The forecast's what-if vocabulary and answers, its projected income and projection, all S3 (the what-ifs by decision 12, the rest by item 130 (3) (b)); `con2` put the next BMI and the committed spend on the wire for `econ`; `WhatIf` still reserves 1 to 15 **`econ` #92 (item 135) fills the next BMI and the committed spend, and rewords the gateway's marker to the register's grammar, naming the owner at S3; moved here from S1 with its ID by item 135.** | `crates/gateway/src/surface/knowledge.rs` `get_economy_forecast` ("What-ifs"); `gateway.proto` `GetEconomyForecastResponse` and `WhatIf` | w6 notes A6 |
| S3-20 | The `on` sphere test's residual: a named or `covered {}` vent is tested against the target sphere first, but an unplaced beacon's target sphere (a `place_beacon`'s `initial`) may reach past every sphere the seat holds, and inside it the structure tests read the whole footprint; S3's fog work bases these reads on the seat's knowledge | `crates/sim/src/targeting.rs` module doc ("The sphere first") | item 135 (2) (c) |
| S3-21 | `get_economy_forecast`'s `bmi_next_dollars` is answered in a Push, where the band it reads hints at opponents' held value; accepted while S1 is unfogged, and withheld during a Push when S3's fog lands | `crates/gateway/src/surface/knowledge.rs` `get_economy_forecast` doc (`bmi_next_dollars`) | item 135 (2) (g) |
| S3-22 | A structure already paid for and still going up when a later edit protects its ground is left as it stands while the area holds: nothing finishes, refunds or ruins it, and the spec says nothing either way | `crates/sim/src/mandate.rs` `next_unfinished_target` | item 136 |
| S5-04 | One expansion at most per goal **`oper` #96 (item 136) deleted `site_for` and its expansion heuristics, so Easy, like a human seat, reaches only the features a site covers; stepping-stone expansion comes with `toward`, which decision 3 (item 128) deferred to S3. Moved here from S5 with its ID by item 136 (3) (q).** | `crates/operator/src/candidates.rs` module doc (`site_for` deleted) | item 113 (7) |

### Tuning, delegated

- `rules/README.md`'s Settled? table, the rows it marks S3 and S3's exit: 5 rows (7 `rules.proto`
  fields: `match.decision_tick_ms`, `structures.resonance_spire`, `verifier.handler_cooldown_min_ms`,
  `max_fires_max`, `notebook_max_chars`, `reach_memory_ms` at S3 and `size_budget_units` at S3's
  exit), plus `locomotion.climb_surcharge`, whose README row says "owner, S3" without the marker and
  whose `rules.proto` field carries none; `crates/sim/src/rules.rs` marks it. `crates/verifier/src/
  limits.rs` restates `reach_memory_ms` as "S2/S3".

---

## S4

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S4-01 | Whether the outside of the map lights its interior, the first time a map is open at its rim | `crates/mesher/src/bake.rs` `LightField::light_at` | — |
| S4-02 | `MAP_COLUMNS_MAX` = 1024 × 1024, with the map sizes **Discharged by `oper` #96 (item 136): `MAP_COLUMNS_MAX` is deleted with `terrain.rs`'s view-chunk decoder.** | `crates/operator/src/easy.rs` `MAP_COLUMNS_MAX` | — |
| S4-03 | The blueprint catalogue: `blueprint_id` is a string, and only the Generator compiles; and a structure's rotation, stored, hashed and snapshotted by `build` #97 and read by nothing while every S1 structure is one column, is drawn by the view and the client when a footprint first spans more than one (item 136 (3) (t)) | `crates/verifier/src/projection.rs` `BLUEPRINTS` (moved from plan-core's `structure_row` by `proj` #85, item 133); `crates/sim/src/interpreter.rs` `compile_build_target`; `crates/sim/src/snapshot.rs` `structure_rotation`; `playbook.proto` `QueueStructureRow.blueprint_id`, `BuildTarget.blueprint_id` | plan T1 |
| S4-04 | The award catalogue: the award fund is computed and paid to nobody; the kill-credit per-seat totals become hashed state when awards read them | `crates/sim/src/economy.rs` `award_fund`; `crates/sim/src/credit.rs` module doc | — |
| S4-05 | The terrain's own shape: two noise octaves' lattice edges and amplitudes, the skin depth, the floor and the headroom | `crates/sim/src/mapgen.rs` `TERRAIN_COARSE_SHIFT`, and by pointer `TERRAIN_COARSE_AMPLITUDE`, `TERRAIN_FINE_SHIFT`, `TERRAIN_FINE_AMPLITUDE`, `TERRAIN_SKIN_VOXELS`; `terrain` (the floor) | plan decision 14 |
| S4-06 | `ZONE_JITTER_VOXELS` = 4 | `crates/sim/src/mapgen.rs` `ZONE_JITTER_VOXELS` | — |
| S4-07 | `CONTESTED_ATTEMPTS` = 64 | `crates/sim/src/mapgen.rs` `CONTESTED_ATTEMPTS` | — |
| S4-08 | `SECTOR_HALF_WIDTH`, a sixth of a turn | `crates/sim/src/mapgen.rs` `SECTOR_HALF_WIDTH` | — |
| S4-09 | A free zone placement with a separation search, and the spec's three-way symmetry | `crates/sim/src/mapgen.rs` `zone_centres` | — |
| S1-27 | `VENT_PATCH_RADIUS` = 1: a vent is a three-by-three patch. **Re-owned by `mine` #86 to S4's map re-derivation per decision 13 (item 133); moved here from S1 with its ID.** | `crates/sim/src/mapgen.rs` `VENT_PATCH_RADIUS` | — |
| S1-28 | A seam's shape: `SEAM_VOXELS_PER_COLUMN` = 4 and `SEAM_DISC_RADIUS` = 5. **Re-owned by `mine` #86 to S4's map re-derivation per decision 13 (item 133); moved here from S1 with its ID.** | `crates/sim/src/mapgen.rs` `SEAM_VOXELS_PER_COLUMN`, `SEAM_DISC_RADIUS` | — |
| S1-29 | `STARTING_FEATURE_CLEARANCE` = 1. **Re-owned by `mine` #86 to S4's map re-derivation per decision 13 (item 133); moved here from S1 with its ID.** | `crates/sim/src/mapgen.rs` `STARTING_FEATURE_CLEARANCE` | — |
| S1-30 | The starting force's two-voxel spacing, its footprint. **Re-owned by `mine` #86 to S4's map re-derivation per decision 13 (item 133); moved here from S1 with its ID.** | `crates/sim/src/mapgen.rs` `RING` | — |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S4-10 | The radio message vocabulary: `BroadcastStep`'s bodies and the `Mailbox` a seat reads | `playbook.proto` `BroadcastStep`; `crates/sim/src/seams.rs` `Mailbox` | plan T1 |

### Tuning, delegated

- `rules/README.md`'s Settled? table, the rows it marks S4: 1 row (`economy.award_fund_percent_of_bmi`).

---

## S5

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S5-01 | A scenario's `safe` seat files the gateway's fallback, not an operator-made safe playbook | `crates/gamectl/src/scenario.rs` `SeatKind::Safe` | plan T18b |
| S5-02 | A `builtin` seat plays Easy, and the `operator` scenario key waits for a second difficulty | `crates/gamectl/src/scenario.rs` `SeatKind::Builtin`; `crates/gamectl/src/scenario/run.rs` module doc "A builtin seat plays Easy"; unmarked copy `xtask/src/scenario.rs` `SEAT_KEYS` | item 113 (8) |
| S5-03 | No spectator token is minted; decided with the built-in-only match flow | `crates/gateway/src/serve.rs` `mint_tokens` doc | T16a notes, section D |
| S5-05 | The way back is costed as the way there | `crates/operator/src/candidates.rs` `score` (first) | — |
| S5-06 | "Near" is one sphere radius, reused as a threat radius | `crates/operator/src/candidates.rs` `score` (second) | — |
| S5-07 | Whether and how Easy uses its (match, seat, round) seed: top-k as best-of-k with no draw | `crates/operator/src/compose.rs` `seed_note` | decision C15 |
| S5-08 | `EASY_CANDIDATES` = 30 | `crates/operator/src/easy.rs` `EASY_CANDIDATES` | spec section 14 |
| S5-09 | `EASY_TOP_K` = 3 | `crates/operator/src/easy.rs` `EASY_TOP_K` | decision C15 |
| S5-10 | `EASY_ROUTE_FILL_PERCENT` = 70 | `crates/operator/src/easy.rs` `EASY_ROUTE_FILL_PERCENT` | spec section 14 |
| S5-11 | `EASY_REPAIRS` = 4 | `crates/operator/src/easy.rs` `EASY_REPAIRS` | spec section 14 |
| S5-12 | The Balanced weights: all ones, `DOLLARS_PER_KW` = 10, `EXPANSION_POINTS_PER_BEACON` = 100, `RISK_POINTS_PER_ENEMY` = 100 | `crates/operator/src/easy.rs` `BALANCED`, `DOLLARS_PER_KW`, `EXPANSION_POINTS_PER_BEACON`, `RISK_POINTS_PER_ENEMY` | plan T18 |
| S5-13 | Which templates the operator suggests for a human, and that a suggestion names own beacons and fixed targets only **Reworded by `oper` #96 (item 136): a suggestion names own beacons and named features only.** | `crates/operator/src/easy.rs` `suggestions` | item 111, section D |
| S5-14 | Spec section 14's Survey post, Defend guard, chatter and target spread are not built at Easy | `crates/operator/src/lib.rs` module doc | item 111, A6 |
| S5-15 | Whether the `WorkCounter` becomes hashed state | `crates/sim/src/seams.rs` `WorkCounter` | — |
| S5-16 | `HARNESS_WORK_BUDGET` = 1 000 000 | `crates/sim/src/world.rs` `HARNESS_WORK_BUDGET` | — |
| S1-17 | `SAFE_REACH_MS` = 60 000: spec section 14's "within 60 s travel", with "at risk" undefined there **Ruled 60 s by decision 14 (item 128); `oper` #96 rewords the marker to owner S5, with the operator's rows, since no rules row holds the operator's values. Moved here from S1 with its ID by item 136.** | `crates/operator/src/easy.rs` `SAFE_REACH_MS` | — |
| S1-18 | `SAFE_MAX_RAISED` = 2: spec section 14's "up to 2" **Ruled 2 by decision 14 (item 128); `oper` #96 rewords the marker to owner S5, with the operator's rows. Moved here from S1 with its ID by item 136.** | `crates/operator/src/easy.rs` `SAFE_MAX_RAISED` | — |
| S1-19 | `SAFE_ESTIMATES` = 4, a bound the spec does not state **Ruled 4 by decision 14 (item 128); `oper` #96 rewords the marker to owner S5, with the operator's rows. Moved here from S1 with its ID by item 136.** | `crates/operator/src/easy.rs` `SAFE_ESTIMATES` | — |

---

## S6

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S6-01 | `LOAD_REFUSALS` is the client's reading of the decode family; a catalogue column would make it the verifier's | `crates/client-gdext/src/editor.rs` `LOAD_REFUSALS` | — |
| S6-02 | One `editor` draft per seat, and a resumed Lull opening `carried` rather than the seat's own draft; named drafts and a draft browser | `crates/client-gdext/src/editor.rs` `EDITOR_DRAFT_ID`, `carry_forward` | plan T19 |
| S6-03 | Ready is final for the rest of the Lull; an un-ready toggle is the real lobby's | `crates/client-gdext/src/rig.rs` `Rig::ready_sent` | — |
| S6-04 | The meter's refresh cadence and layout, "with the real lobby". **Nearest section:** the real lobby is S6's (S6-03) | `crates/client-gdext/src/rig.rs` module doc; `godot/scripts/editor.gd` header | w6 notes A4 |
| S6-05 | The surface format's vertex layout (art pass) | `crates/client-gdext/src/surface.rs` module doc | item 54 |
| S6-06 | The seam and vent colours, and whether richness is visible (art pass) | `crates/client-gdext/src/view.rs` `palette_of` | — |
| S6-07 | One save per match, latest wins; a save browser | `crates/gateway/src/save.rs` module doc | w6 notes D question 5 |
| S6-08 | `MAX_DRAFT_LABEL_CHARS` = 120 | `crates/gateway/src/surface/planning.rs` `MAX_DRAFT_LABEL_CHARS` | — |
| S6-09 | The mesher's `PALETTE`, spike G1's colours (art polish) | `crates/mesher/src/sweep.rs` `PALETTE` | — |
| S6-10 | `FACE_SHADE_256`, G1's factors (art) | `crates/mesher/src/sweep.rs` `FACE_SHADE_256` | — |
| S6-11 | `AMBIENT_256` = 64 and `LIGHT_SPAN_256` = 192 (art polish) | `crates/mesher/src/sweep.rs` `AMBIENT_256`, and by pointer `LIGHT_SPAN_256` | — |
| S6-12 | The typed parameter catalogue (type, unit, range, choices; numbers 3 to 15), and a value's unit display | `crates/plan-core/src/library.rs` `instantiate`; `gateway.proto` `InstantiateTemplateRequest.Parameter`, `FilledParameter.value`; `playbook.proto` `TemplateParameter`; `godot/scripts/wizard.gd` header | w6 notes A6 |
| S6-13 | The one project-wide string table: who assembles it, where it lives, its file and format, and its wording | `crates/plan-core/src/strings.rs` module doc; `crates/verifier/src/strings.rs` module doc; `godot/scripts/strings.gd` header; `crates/operator/src/compose.rs` `why_for`; `gateway.proto` `InstantiateTemplateResponse.why` | plan T8, T19 |
| S6-14 | The Pall's sky, ambient and glow (art pass) | `godot/scenes/vista.tscn` `Environment_pall` | — |
| S6-15 | The glare's direction, colour and energy (art pass) | `godot/scenes/vista.tscn` node `Glare` | — |
| S6-16 | The editor panel's layout, sizes and colours, the menu's wording and the ghost's look | `godot/scripts/editor.gd` header | plan T19 |
| S6-17 | `PICK_PIXELS` = 28 | `godot/scripts/editor.gd` `PICK_PIXELS` | — |
| S6-18 | `PANEL_WIDTH` = 380 | `godot/scripts/editor.gd` `PANEL_WIDTH` | — |
| S6-19 | `ROUTE_COLOUR` and the ghost's colours (art) | `godot/scripts/editor.gd` `ROUTE_COLOUR` | — |
| S6-20 | `ROUTE_LIFT` = 1.2 (art) | `godot/scripts/editor.gd` `ROUTE_LIFT` | — |
| S6-21 | `user://`'s remembered match is a per-machine convenience, not state; the save browser | `godot/scripts/lobby.gd` header | w6 notes A4 |
| S6-22 | Forgetting an ended match, and the New/Resume layout, including what a refused Resume leaves (New match unreachable until a restart) | `godot/scripts/lobby.gd` header | items 113 (11), 114 (5) |
| S6-23 | `EVENT_ROWS` = 14 | `godot/scripts/lobby.gd` `EVENT_ROWS` | — |
| S6-24 | Accessibility polish beyond generated names (font scaling, reduced motion, focus order) | `godot/scripts/rows.gd` header; `godot/scripts/wizard.gd` header | plan T19 |
| S6-25 | `ICON_COLOURS` (art pass) | `godot/scripts/rows.gd` `ICON_COLOURS` | — |
| S6-26 | `ICON_SIZE` | `godot/scripts/rows.gd` `ICON_SIZE` | — |
| S6-27 | `SENTENCE_WIDTH` = 300 | `godot/scripts/rows.gd` `SENTENCE_WIDTH` | — |
| S6-28 | `ROW_GAP` = 12 | `godot/scripts/rows.gd` `ROW_GAP` | — |
| S6-29 | `LINE_WIDTH` = 340 | `godot/scripts/rule_list.gd` `LINE_WIDTH` | — |
| S6-30 | Entity markers' shapes and colours (art) | `godot/scripts/vista.gd` header | plan T16 |
| S6-31 | `OWN_COLOUR` and the other seat's and nobody's colours (art) | `godot/scripts/vista.gd` `OWN_COLOUR` | — |
| S6-32 | `BEACON_GLOW` (art) | `godot/scripts/vista.gd` `BEACON_GLOW` | — |
| S6-33 | `TEXT_WIDTH` = 320 | `godot/scripts/wizard.gd` `TEXT_WIDTH` | — |
| S6-34 | `MARK_COLOUR` (art pass) | `godot/scripts/wizard.gd` `MARK_COLOUR` | — |
| S6-35 | `REFUSAL_COLOUR` (art pass) | `godot/scripts/wizard.gd` `REFUSAL_COLOUR` | — |
| S6-36 | `PAGE_GAP` = 10 | `godot/scripts/wizard.gd` `PAGE_GAP` | — |
| S6-37 | The wizard's page layout: all pages in one column rather than one page at a time (a value's unit display is S6-12's) | `godot/scripts/wizard.gd` header | w6 notes A4 |
| S6-38 | `meta.parameters` on a PLAYBOOK round-trips unread; whether the verifier refuses it with a code of its own | `playbook.proto` `Meta.parameters` | w6 notes D |
| S1-40 | The lobby's match: the golden seed, two seats with the human at seat 0 and Easy at seat 1, the rules' segment ladder, and the round limit; the settings screen and the Probation preset. **With T22a:** `ROUND_LIMIT` becomes 3 and the comment records item 123 (2) 3, the settings screen and the Probation preset staying the owner's at S1. Cross-referenced from D-20 **`ui` #98 (item 136) rewords the marker to "the match the lobby hosts — OWNER, S6, with the settings screen", since item 127 (10) deferred the settings screen. Moved here from S1 with its ID by item 136.** | `godot/scripts/host_link.gd` `MATCH_SEED` and the constants after it | item 123 (2) 3 |
| S6-40 | `DORMANT_CODES` (E0601, W0603), the codes for which the editor shows E0601's `allow_dormant_beacons` checkbox, is the client's reading of the diagnostic catalogue; a catalogue column naming the codes an option answers would make it the verifier's, as S6-01 says of `LOAD_REFUSALS` | `crates/client-gdext/src/editor.rs` `DORMANT_CODES` | item 136 |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S6-39 | The lobby's whole layout, a strip of buttons and a text column over the vista | `godot/scripts/lobby.gd` header | — |

### Tuning, delegated

- `rules/README.md`'s Settled? table, the rows it marks S6: 2 rows (`mesher.surfaces_per_frame` and
  `bytes_per_frame`, "S6 art pass"; `light_max` and `light_atten`, "S6's art polish"), plus
  `mesher.age_frames`, whose `rules.proto` source says "owner, at S6's art pass" and whose README row
  lost both the owner and the stage (4 marked `rules.proto` lines, 5 fields). Two restatements disagree with
  their source: `crates/sim/src/rules.rs`'s `mesher_drain_surfaces` and `mesher_drain_bytes` lost
  "S6", and `crates/mesher/src/upload.rs`'s module doc gives K and B to S2's exit.
  `crates/mesher/src/bake.rs`'s module doc restates the light pair.

---

## S7

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| S7-01 | The digest's payload beyond the window, the per-kind counts and the prose | `crates/gateway/src/feed.rs` `Digest` | item 97; plan T13 |
| S7-02 | The replay's event log, reader, scrub and recap use | `crates/gateway/src/save.rs` module doc | w6 notes D |
| S7-03 | The last recap's events do not survive a resume | `crates/gateway/src/surface.rs` module doc | w6 notes D |
| S7-04 | `ViewEntity.at` in whole voxels, not recording-grade positions | `gateway.proto` `ViewEntity.at`; `GetViewResponse.entities` (its second half) | T16a notes, section D |

---

## Hardening

### The owner at hardening

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| H-01 | `PACER_PERIOD_US` = 100 ms | `crates/client-gdext/src/pacer.rs` `PACER_PERIOD_US` | T16a notes, B6 |
| H-02 | `CLOCK_REPORT_US` = 250 ms | `crates/client-gdext/src/pacer.rs` `CLOCK_REPORT_US` | T16a notes, A (2) |
| H-03 | `KEEPALIVE_US` = 5 s, with `READ_TIMEOUT` | `crates/client-gdext/src/pacer.rs` `KEEPALIVE_US` | T16a notes, B6 |
| H-04 | `SEAT_CALLS_PER_REFILL` = 6; the margin is the client's, the numbers follow the gateway's | `crates/client-gdext/src/rig.rs` `SEAT_CALLS_PER_REFILL` | item 123 (1) |
| H-05 | A resumed Lull restarts its timer in full, so quitting in a Lull buys planning time | `crates/client-gdext/src/rig.rs` `observe`; `crates/gateway/src/surface.rs` `Surface::resume` doc | items 99, 114 (5) |
| H-06 | `IN_PROCESS_LIMITS` against `EASY_CALL_BUDGET` | `crates/gamectl/tests/operator.rs` `easy_fits_the_in_process_limits` | item 111, section D |
| H-07 | Which detail rung an omitted `detail` means (`standard`) | `crates/gateway/src/detail.rs` `DEFAULT` | — |
| H-08 | The detail ladder (16 / 64 / 256 events) and its ceiling `MAX_PAGE_EVENTS` = 256 | `crates/gateway/src/detail.rs` `events`; `crates/gateway/src/feed.rs` `MAX_PAGE_EVENTS` | — |
| H-09 | `MAX_MESSAGE_BYTES` and `MAX_FRAME_BYTES` = 256 KiB | `crates/gateway/src/frame.rs` `MAX_MESSAGE_BYTES`, and by pointer `MAX_FRAME_BYTES` | plan T9 |
| H-10 | The rate limits: `CALLS_PER_TICK` = 8, `CALLS_PER_WINDOW` = 600, `WINDOW_TICKS` = 200 | `crates/gateway/src/limit.rs` `CALLS_PER_TICK`, and by pointer `CALLS_PER_WINDOW`, `WINDOW_TICKS` | plan T9 |
| H-11 | Remove the unused `DEFAULT_PORT` | `crates/gateway/src/net.rs` `DEFAULT_PORT` | items 117 (3), 121 (1) |
| H-12 | A save from an older build is refused, with no converter | `crates/gateway/src/save.rs` module doc ("What it holds") | w6 notes D |
| H-13 | `READ_TIMEOUT` = 30 s | `crates/gateway/src/session.rs` `READ_TIMEOUT` | — |
| H-14 | `MAX_WAIT_MS` = 60 000 | `crates/gateway/src/surface.rs` `MAX_WAIT_MS` | — |
| H-15 | `MAX_ADVANCE_MS` = 60 000 | `crates/gateway/src/surface/control.rs` `MAX_ADVANCE_MS` | T16a notes, section D |
| H-16 | `MAX_CLOCK_STEP_MS` = 60 000 | `crates/gateway/src/surface/control.rs` `MAX_CLOCK_STEP_MS` | T16a notes, section D |
| H-17 | `list_beacons`' ladder (8 / 24 / 40) and the what-if budget | `crates/gateway/src/surface/knowledge.rs` `beacon_budget`, and by pointer `what_if_budget` | item 63 |
| H-18 | `MAX_WAYPOINTS` = 64 | `crates/gateway/src/surface/knowledge.rs` `MAX_WAYPOINTS` | — |
| H-19 | `MAX_DRAFTS` = 32 | `crates/gateway/src/surface/planning.rs` `MAX_DRAFTS` | — |
| H-20 | `MAX_PLAYBOOK_CHARS` = 512 KiB | `crates/gateway/src/surface/planning.rs` `MAX_PLAYBOOK_CHARS` | — |
| H-21 | `TOKEN_LIFETIME_TICKS` = the whole match | `crates/gateway/src/token.rs` `TOKEN_LIFETIME_TICKS` | plan T9 |
| H-22 | `VIEW_PAGE_BYTES` = 256 KiB | `crates/gateway/src/viewfeed.rs` `VIEW_PAGE_BYTES` | T16a notes, section D |
| H-23 | `MAX_CONNECTIONS` = 8, counted at accept | `crates/gateway/src/serve.rs` `MAX_CONNECTIONS` | T16a notes, section D |
| H-24 | `PAGES_MAX` = 4 | `crates/operator/src/easy.rs` `PAGES_MAX` | — |
| H-25 | `EVENT_BUS_CAPACITY` = 512 | `crates/sim/src/events.rs` `EVENT_BUS_CAPACITY` | item 63 |
| H-26 | `RECONNECTS` = 5 | `godot/scripts/host_link.gd` `RECONNECTS` | — |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| H-27 | The zips' compression: every entry is stored | `xtask/src/zip.rs` module doc | items 117 (9), (14) |

---

## The v0.1 gate

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| G-01 | The public repository URL (`example.invalid`; `NOASSERTION`). **Nearest section:** the entries say "before the first public release" and "once the GitHub org exists"; a draft prerelease published sooner needs it sooner | `Cargo.toml` `[workspace.package] repository`; `REUSE.toml` `SPDX-PackageDownloadLocation`; unmarked copies: `packaging/README.md` "## PLACEHOLDERs" first bullet, `docs/LICENSING.md` | item 117 (12) |
| G-02 | How `VERIFIER_VERSION` is derived once there are releases | `crates/verifier/src/lib.rs` `VERIFIER_VERSION` | — |
| G-03 | The export preset's product name and file description are the lockup until the name is confirmed. **Nearest section:** "with the org" | `godot/README.md` "The export (T21)"; unmarked pointer: `packaging/README.md` "## PLACEHOLDERs" second bullet | item 117 (6) |
| G-04 | The export preset's icon, company and copyright fields are empty. **Nearest section:** "before the owner publishes a release" | `godot/README.md` "The export (T21)" | item 117 (6) |

---

## v1.1 and later

### The owner

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| V-01 | Whether the config and announce lines become part of anything published | `crates/gateway/src/serve.rs` module doc | T16a notes, section D |
| V-02 | The program id catalogue becomes a contract when the script arm opens (v1.2) | `crates/sim/src/programs.rs` `Program::id`; `proto/gp/v1/seams.proto` `BuiltinProgram.program_id` | — |
| V-03 | Whether `gamectl`'s doctor details and scenario diagnostics belong in a string table at translation time. **Nearest section:** "the stage that first wants a second language", past v1 (AGENTS.md section 11) | `crates/gamectl/src/strings.rs` module doc | — |

### No one named

| ID | What is guessed | Anchors | Source |
|---|---|---|---|
| V-04 | A second `buf breaking` comparison against the last release tag once v1.1 publishes `gp.api.v1` | `xtask/src/main.rs` `step_buf` | AGENTS.md section 9 item 8 |

---

## Stale: names a closed task

Each entry's only "when" is a task that has closed. The stage and the who are this register's
proposal. Stale copies that belong to a live row are listed in that row instead (S1-13, S1-26,
S1-42).

### The owner

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| X-01 | The real fix for `CHAIN_RESERVE_CAP`: a `length_ms` cap in the scenario format, "in this task's pull request" (T15) | `crates/gamectl/src/scenario/run.rs` `CHAIN_RESERVE_CAP` | owner, S1, with S1-41 (both touch the format, an `xtask` contract path) | plan T15 |
| X-03 | The standing sentence, "OWNER/T14 fills the sentence in with the economy" **Discharged by `econ` #92 (item 135): `get_briefing`'s standing is the seat's audit score and displayed rank from the sim's final audit, and the sentence says so. The displayed rank restates the audit's tie-break in `rank_among` until the sim has a public read (`build`).** | `crates/gateway/src/strings.rs` `standing` | owner, S1, with the recap's settlement lines | plan T14 |
| X-04 | Whether `normals` are uploaded at all, "T12's decision … owner signs it off then" | `crates/mesher/src/lib.rs` `MeshBuffers` | owner, S6's art pass, with S6-05 | plan T12 |
| X-05 | Whether a deploy pays the visit handshake, "the owner settles it at T14". **Discharged by `proj` #85 per decision 15 (item 133): a deploy pays no handshake.** | `crates/verifier/src/interface.rs` `place_beacon_ms` (moved from plan-core by `proj` #85) | owner, S1, with the full mandate contract (S1-23) | plan T14 |
| X-08 | The winner of a round-limit or no-survivor end is decided by the final audit, "(owner, at T14)"; both carry `winner: None` | `crates/sim/src/runner.rs` `MatchOutcome` | owner, S1, with the audit | plan T14 |
| X-09 | The recap's settlement is not done in `end_recap`, "(owner, at T14)" | `crates/sim/src/runner.rs` `end_recap` | owner, S1, with the recap's settlement lines (w6 notes A6) | plan T14 |
| X-12 | The `$` a placed beacon costs is not charged in `place_beacon`, "(owner, at T14)". Nothing else charges it either: `World::beacon_cost` is read only for a recycle's refund, held value and kill credit, and the interpreter's deploy (`crates/sim/src/interpreter/exec.rs`) calls `place_beacon` and pays nothing, so a placed beacon is free at the skeleton | `crates/sim/src/world.rs` `place_beacon` | owner, S1, with the Quartermaster | plan T14 |
| X-14 | The six error codes are a proposal, ratified "at the latest when T13 fills the gateway payloads" | `proto/gp/api/v1/gateway.proto` `Code` | owner, S1's first contract pull request | plan T13 |
| X-15 | `GetBriefingResponse`'s structured payload, "Filled by T13" (5 to 15 still reserved) | `gateway.proto` `GetBriefingResponse` | owner, S1's first contract pull request: name the stage that fills it | plan T13 |
| X-16 | `GetRecapResponse`'s settlement lines, "Filled by T13 once T14's economy produces them" **Discharged by `econ` #92 (item 135): `get_recap` fills `Settlement` (`bmi_dollars` from the `settled` event, `band_rank` and `band_percent` from the sim's `economy::ladder_place` and `band_percent`, `award_dollars` 0 until S4) and `Shortfall` (`kw` from `power::dark_load(..).shed()`, the dark beacons as `b_NN`).** | `gateway.proto` `GetRecapResponse` | owner, S1 (w6 notes A6) | plan T13 |
| X-17 | `GetBeaconResponse`'s per-beacon detail, "Filled by T13" | `gateway.proto` `GetBeaconResponse` | owner, S1's first contract pull request: name the stage that fills it | plan T13 |
| X-18 | `Event`'s per-kind structured payload, "Filled by T13" | `gateway.proto` `Event` | owner, S1's first contract pull request: name the stage that fills it | plan T13 |

### The owner at hardening

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| X-02 | The per-method salience order, "OWNER settles it with T13" | `crates/gateway/src/detail.rs` module doc | owner, hardening, with H-07 and H-08 | plan T9, T13 |

### A named lane

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| X-06 | The sighting catalogue grows: "kill credit and the economy at T14 and radio at S4" | `crates/sim/src/knowledge.rs` module doc | the sim lane of each producing stage; radio at S4 | plan T5, T14 |
| X-07 | The router key's beacon half is `BeaconId::NONE`, "owner, at T11" | `crates/sim/src/pathing/router.rs` module doc | the next sim lane, S1: check whether T11 and T14 gave it a value, and reword | plan T11 |
| X-10 | An intent carries only its addressing: "the intent vocabulary is T11's" | `crates/sim/src/seams.rs` `Intent::kind` | the next sim lane, S1: reword or delete | plan T11 |
| X-11 | The `PlaybookInterpreter` signature, "(owner, at T11)" | `crates/sim/src/seams.rs` `PlaybookInterpreter` | the next sim lane, S1: reword or delete | plan T11 |
| X-13 | "T8 (plan-core) owns the text-preserving layer"; if the canonical encoder must emit a written default, the owner decides | `examples/README.md` "Open points for the owner" | resolved by T8's round-trip goldens; the examples lane rewords it | item 74 |

---

## Malformed: no who or no when

### The owner

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| M-01 | `win_rate_percent` is an integer field on the scenario result (no who, no when) | `.github/workflows/nightly-scenarios.yml` the alarm-band step | owner, S2, with S2-01 | item 116 (6)(j) |
| M-02 | `WALLED_PACKAGES` names `presentation` and `solve`, which do not exist (no who, no when) | `clippy.toml` header wall list; `xtask/src/main.rs` `WALLED_PACKAGES` | owner, whenever either crate is proposed, as a section 5 change, or never (plan T0) | plan T0 |
| M-03 | The `disallowed-methods` entries held back until their paths resolve (no who, no when) | `clippy.toml` held-back list | owner, S1's first contract pull request | — |
| M-05 | The scenario assertion vocabulary: the owner confirms that taking none of item 97's three is right, or names one (no when) | `crates/gamectl/src/scenario.rs` `ASSERTIONS` | owner, at this demo's review or S1 with S1-41 | item 97; plan T3 |
| M-06 | `deny.toml`'s five empty slots: licence carve-outs, `[[licenses.clarify]]`, accepted duplicates, dev-only skip-trees, accepted advisories (fill-in slots, no who, no when) | `deny.toml` `[licenses]` exceptions and clarify, `[bans]` skip and skip-tree, `[advisories]` ignore | owner, as each case arises (a section 5 change) | — |
| M-07 | Marker easing: `EASE_RATE` = 6 and `TURN_AFTER` = 0.05 ("Tuning", no who in the constants, no when) | `godot/scripts/vista.gd` `EASE_RATE`, `TURN_AFTER` | owner, S6, with S6-30 | T16a notes, B6 |
| M-08 | No BSR module is named (no who, no when) | `proto/buf.yaml` comment above `lint:` | owner, v1.1, if the schemas are ever pushed to a registry | — |
| M-09 | `Event.kind` is a string until the catalogue stops moving (no who, no when) | `gateway.proto` `Event.kind` | owner, after S4, the last stage the entry names as adding kinds | — |
| M-10 | `Options` is a stub; the one option the spec pins is "allow dormant beacons" (no who, no when). **Discharged by `proj` #85 (item 133): `Options` stays `allow_dormant_beacons` alone, held by an exhaustive-pattern test in `crates/verifier/src/estimate.rs`.** | `playbook.proto` `Options` | owner, S1, with the grid (spec section 7) | plan T1 |
| M-11 | The interface row catalogue is not enumerated (no who, no when) | `playbook.proto` `InterfaceStep` | owner, S1, with the section 6 mandate contract | plan T1 |
| M-12 | An area is an axis-aligned box (a polygon or a sphere would be an owner decision; no when) | `playbook.proto` `Area` | owner, S3, with the pickers | — |
| M-13 | A patrol route is a list of locations (no who, no when) | `playbook.proto` `FallbackPatrol` | owner, S3, with the rest of the vocabulary | — |
| M-14 | The mandate messages and the five writs' ids belong in `gp/v1/mandate.proto` when section 6's contract is written (no when) | `playbook.proto` `MandateSettings`; `proto/gp/v1/seams.proto` `BuiltinMandate.builtin_id` | owner, S1, with the section 6 mandate contract (S1-39) | — |
| M-15 | `BuiltinOperator.builtin_id` is a string until section 14's operator catalogue is written (no who, no when) | `seams.proto` `BuiltinOperator` | owner, S5, with Normal and Hard | — |
| M-16 | Whether the mapgen golden should carry `(seed, seats)` lines, "when a later stage wants a map digest without a match behind it" (no stage) | `tests/golden/mapgen/README.md` "The format" | owner, S4, which re-derives map generation | — |

### The owner at hardening

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| M-04 | `BACKLOG_BOUND_MS` = 5 000 ("Tuning, OWNER", no when) **`ui` #98 (item 136) gives the marker its when, "OWNER, at hardening, with the pacer's period", this row's proposal.** | `crates/client-gdext/src/pacer.rs` `BACKLOG_BOUND_MS` | owner, hardening, with H-01 | T16a notes, B6 |

---

## Unmarked: guesses the grep cannot see

Seven rows, each added by hand because no line says PLACEHOLDER:

### The owner

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| U-04 | `interface_times.*` are spec section 5's values, and "all of section 5 is marked Tuning", with no who or when. **Discharged by `proj` #85 per decision 15 (item 133): the times are ratified as section 5's; the Settled? cell is reworded to "ratified, decision 15" by the next lane that touches `rules/`.** | `rules/README.md` Settled? row `interface_times.*` | owner, S1, with the full mandate contract | plan T11 |
| U-05 | No pause button, and whether a minimised window keeps pacing (a stalled client is a pause) | none in the code; the T16a notes, section D | owner; no stage proposed | T16a notes, section D |
| U-06 | Entity ids are opaque per-viewer handles; revisited if v1.1 agents or S7's recordings need two seats to agree | none in the code; the T16a notes, section D | owner, v1.1 | T16a notes, section D |
| U-07 | The full predicate catalogue is S3's, and the plan asked `playbook.proto` to say so in a stub; no stub names it | `proto/gp/v1/playbook.proto` (no entry) | owner, S3 | plan T1 |

### The owner at hardening

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| U-02 | Unauthenticated calls are not counted; a per-connection budget the session holds would need per-connection state ("OWNER, at hardening, with the numbers") | `crates/gateway/src/limit.rs` module doc "What it does not count" | owner, hardening | — |

### A named lane

| ID | What | Anchors | Proposed | Source |
|---|---|---|---|---|
| U-01 | `.gitignore`'s "Placeholder paths: the real on-disk location is decided when the host is built", stale since `crates/gateway/src/cache.rs` fixed the location. **T22a rewrites it** | `.gitignore` the private-cache comment | resolved by T22a | item 98; item 123 (3) |
| U-03 | The casual fog policy has no phase term, so a casual match is unfogged in the Lull too: a known defect on `main` with no marker | `crates/gateway/src/fog.rs` `FogPolicy::unfogged` | hardening or the next gateway lane | items 107 (6), 116 (6)(k) |

### Copies of the rows above that a plain search misses

Each belongs to a row above and is not counted again; a plain search misses it or sees it only in
part:

- `packaging/README.md` "## PLACEHOLDERs": the heading is a grep hit (a reference) and its two
  bullets are unmarked: the repository's public address (G-01) and the export preset's fields (a
  pointer to G-03 and G-04).
- `xtask/src/scenario.rs` `SEAT_KEYS`: "owner, at S5" with no marker (S5-02).
- `crates/sim/src/lib.rs` `DETERMINISM_MATCH_SEED`: "owner, at S1" with no marker (S1-26).
- `godot/scripts/mesher_rules.gd` `RULES_JSON`: the client's inline copy of `match.lull_ms` (D-01).

Not a guess, and stale prose rather than a register entry: the root `README.md`'s "most crates are
still empty placeholders", which T22a rewrites.

---

## Appendix A: cross-check against skeleton-plan section 3

`grep -c '^- \*\*PLACEHOLDERs:\*\*'` counts 28 lines in `docs/design/skeleton-plan.md` at the base
(T0 to T21b) and 30 at `a80fb72` (`main` after item 123's docs pull request, which added T22a's and T22b's). This appendix checks the
30. Each item is **found** (the row that carries it), **resolved** (the decisions-log item that
settled it) or **missing** from the code.

| Task | Item on its PLACEHOLDERs line | Status |
|---|---|---|
| T0 | `clippy.toml` narrows to "`client-gdext` and `mesher` of the four exist" | found, M-02 |
| T1 | `kind` enum membership (decision 5) | resolved, item 76 |
| T1 | the fingerprint's hash function (decision 6) | resolved, item 77 (S1-42 folds it into the sim) |
| T1 | JSON-RPC parameter casing (decision 9) | resolved, item 80 (`gateway.proto`'s tombstone) |
| T1 | Defend and Attack stubs at S2 | found, S2-13 |
| T1 | broadcast bodies at S4 | found, S4-10 |
| T1 | the full predicate catalogue at S3 | missing (U-07) |
| T2 | the CSR cell size | found, S2 delegation (`broadphase.cell_size_voxels`) |
| T2 | `DETERMINISM_TICKS` raised from 1 200 (owner, at T20) | found, S1-26 (moved to S1 by item 116 (6)(e)) |
| T2 | `rust-version` in the workspace manifest | found, S1-01 |
| T2 | `repository` in the workspace manifest | found, G-01 |
| T3 | the scenario assertion vocabulary (owner, at T15) | found, M-05 (T15 took none, item 97) |
| T3 | the screenshot thresholds carried from G1 (re-ratified at T16) | resolved, item 116 (6) (plan decision 22) |
| T4 | K = 4 / B = 512 KiB | found, S6 delegation (`upload.rs` gives S2's exit) |
| T4 | the ageing term | found, S6 delegation (`mesher.age_frames`) |
| T4 | `LIGHT_MAX` / `LIGHT_ATTEN` | found, S6 delegation |
| T5 | vents per map, ore yield per richness, map dimensions | found, D-08 to D-18 and the S1 delegation |
| T5 | the 190 kW ceiling's two PLACEHOLDERs | found, S2 delegation (`kw_per_unit`, `reserve_percent`, `map_ceiling_kw`) |
| T6 | the playbook size budget | found, S3 delegation (`size_budget_units`) |
| T6 | reach memory N | found, S3 delegation (`reach_memory_ms`) |
| T6 | which codes the skeleton omits (decision 18) | resolved, item 93 |
| T7 | `CLIMB_SURCHARGE` = 4 | found, S3 delegation (`climb_surcharge`) |
| T7 | the repath cap 16 | found, S2 delegation (`repath_cap_per_tick`) |
| T7 | the corner entrance per cluster | found, S3-11 |
| T8 | the English string table's home and wording | found, S6-13 |
| T8 | the size meter's budget number (decision 19) | resolved, item 94 (the value is the S3 delegation's) |
| T9 | rate-limit numbers and token lifetime | found, H-10, H-21 |
| T9 | the private match cache's location (decision 20) | resolved, item 98 |
| T9 | the detail budgets | found, H-07, H-08, H-17 |
| T10 | the respawn delay curve | found, S2 delegation (`commander.respawn_*`) |
| T11 | interface times as rules-table data | missing (U-04: the rules row carries no marker) |
| T11 | the condition families the templates do not use stay stubs naming their stage | found: the stubs are `PlanError::NotAtThisStage`'s stage list in `crates/sim/src/interpreter.rs` (`beacon_under_attack` and `most_threatened` at S2, the tag and `ENEMY_KNOWN` filters at S3), which carries no marker; the marked rows are S2-04 and S3-07 |
| T12 | the surface format's vertex layout | found, S6-05 |
| T12 | the `unsafe_code` scoped allow (decision 12) | resolved, item 83 |
| T13 | the digest payload's exact contents | found, S7-01 |
| T13 | the template folder's location on each platform | resolved, item 117 (`LIBRARY_PATH`) |
| T14 | every `$` and `kW` number, the sphere radius, placement range and commander speed | found, D-02 to D-07, D-19, and the S1, S2 and S4 delegations |
| T14b | the key-core netted out of draw | found, S1-31 |
| T14b | whether the brownout order skips a beacon whose shed relieves nothing | found, S1-33 |
| T14b | whether a priority raise re-applies the brownout order | found, S1-22 |
| T15 | the Rusher, Turtle and Hunter playbooks and their seed set, S2; `NIGHTLY_SCENARIOS_ENABLED` unset | found, S2-01 |
| T16 | camera speeds and easing | found, D-21 |
| T16 | every art and audio asset placeholder with its SPDX line and credits entry | found, S6-09 to S6-11, S6-14, S6-15, S6-19, S6-20, S6-25, S6-30 to S6-32, S6-34, S6-35; no audio asset ships |
| T16 | the screenshot tolerance re-ratified (decision 22) | resolved, item 116 (6) |
| T16a | generated terrain served whole; terrain fog's unit and memory | found, S3-05 |
| T16a | unit, structure, Survey and Spire vision; reach memory N | found, D-25, S1-34, S3 delegation |
| T16a | what a seat sees of an enemy asset beyond where it is | found, S2-11 |
| T16a | `MAX_ADVANCE_MS`, the clock step, `VIEW_PAGE_BYTES`, `MAX_CONNECTIONS` | found, H-15, H-16, H-22, H-23 |
| T16a | whole-voxel positions and full entity lists per view | found, S7-04, S2-12 |
| T16a | the config and announce lines as an internal format | found, V-01 |
| T16a | the casual policy's missing phase term | missing (U-03) |
| T16a | what `_status` shows for an untimed Lull and a recap | found, S1-11 |
| T17 | none new | — |
| T18a | `IN_PROCESS_LIMITS` (now, then the numbers at hardening) | found, D-26 (and H-06) |
| T18a | the typed parameter catalogue and a parameter's `type` | found, S6-12 |
| T18a | whether the verifier refuses `meta.parameters` on a PLAYBOOK | found, S6-38 |
| T18a | forecast what-ifs, projection and income | found, S1-46 |
| T18a | enemy beacon detail | found, S2-11 |
| T18a | the live Push readout in a method named "forecast" | found, S1-12 |
| T18a | `LIBRARY_PATH` beside a shipped binary | resolved, item 117 |
| T18 | the Balanced utility weights | found, S5-12 |
| T18 | target spread deliberately absent | found, S5-14 |
| T18 | which three templates ship (decision 10) | resolved, item 81 |
| T18b | the reworded "short" and "at risk" | found, S1-21 |
| T18b | a placed beacon's draw read as net zero in Easy | found, S1-14 |
| T18b | the `operator` scenario key | found, S5-02 |
| T18b | a `safe` seat filing the gateway's fallback | found, S5-01 |
| T18b | Hold & Build's place-and-build shape | resolved, item 119 (9) and (10) |
| T19 | the string table's file and format | found, S6-13 |
| T19 | accessibility polish beyond generated names | found, S6-24 |
| T19 | the segment clock and fits pill | found, S3-16 |
| T20 | none for the alarms | — |
| T20 | the suite's time budget (`ci.yml`'s PLACEHOLDER) | resolved, item 119 (5) (T21b replaced it) |
| T20a | none expected | — |
| T21 | zip contents, version string and tag scheme | resolved, item 117 (2), (10), (13) |
| T21 | the export preset's product name, icon, company and copyright; the repository URL | found, G-03, G-04, G-01 |
| T21b | none expected; each part closes one | — (it closed the templates' three and `ci.yml`'s) |
| T22a | `host_link.gd`'s, reworded (the settings screen and the Probation preset, owner at S1) | found, S1-40 (with T22a) |
| T22a | the camera's tuning values stay, for the owner at the review | found, D-21, D-22, D-23 |
| T22b | none of its own; the grammar and the `xtask` check are S1 contract items | found, S1-10 |

**Totals:** 80 items on the 30 lines (an item is one row of this table): 58 found, 15 resolved, 3
missing from the code, and 4 lines that book none.

### The T16a notes' eight

`docs/design/skeleton-plan-t16a-notes.md`'s T22 paragraph books "T16a's eight" without listing
them. They are read here as the eight items of the plan's T16a PLACEHOLDERs line, checked above: 7
found and 1 missing (the casual phase term, U-03). The notes' other PLACEHOLDER entries:

| Where in the notes | Item | Status |
|---|---|---|
| B, item 6 | the pacer period | found, H-01 |
| B, item 6 | its backlog bound | found, M-04 |
| B, item 6 | the speed set | found, D-24 |
| B, item 6 | easing and derived facing of whole-voxel entities | found, M-07 |
| B, item 6 | the keep-alive interval | found, H-03 |
| B, item 6 | the clock-report cadence | found, H-02 |
| D | is generated terrain secret | found, S3-05 (served whole, item 107 (1)) |
| D | where the Lull's length comes from | found, D-01 (a match setting is S1's) |
| D | pause, and an unfocused window | missing (U-05) |
| D | client death mid-Push | resolved, item 111 (decision C8; D-27) |
| D | manual or automatic saving | resolved, item 111 (automatic at Lull boundaries; S6-07) |
| D | the spectator token | found, S5-03 |
| D | the hosting process and four crate-map sentences | resolved, items 109 and 110 (`gamectl host`, AGENTS.md section 3) |
| D | entity identity across viewers | missing (U-06) |
| D | the transport numbers | found, H-15, H-16, H-22, H-23 |

### The wave-6 notes' own

`docs/design/skeleton-plan-w6-notes.md`'s A5 books "this file's". A1 repeats the plan's T18a line,
checked above.

| Where in the notes | Item | Status |
|---|---|---|
| A2, item 7 (T17) | one save per match, latest wins | found, S6-07 |
| A2, item 7 | where a resume lands | found, D-27 |
| A2, item 7 | a resumed Lull's timer starts full | found, H-05 |
| A2, item 7 | the replay's log, reader, scrub and recap use | found, S7-02 |
| A2, item 7 | the last recap's events after a resume | found, S7-03 |
| A2, item 7 | resuming a save from an older build | found, H-12 |
| A3, item 7 (T18) | "power short" and "at-risk" | found, S1-21 |
| A3, item 7 | the rescue rule | found, S2-02 |
| A3, item 7 | Easy's use of its seed | found, S5-07 |
| A3, item 7 | the Balanced weights | found, S5-12 |
| A3, item 7 | `IN_PROCESS_LIMITS` against `EASY_CALL_BUDGET` | found, H-06 |
| A4, item 5 (T19) | chip editing and drag reordering | found, S3-17 |
| A4, item 5 | a live placement ghost | found, S3-02 |
| A4, item 5 | unit display of parameter values | found, S6-12 |
| A4, item 5 | the meter's layout and refresh cadence | found, S6-04 |
| A4, item 5 | the save browser | found, S6-07, S6-21 |
| A4, item 5 | `user://`'s remembered match id | found, S6-21 |
| D, now 1 | may an in-process seat have its own rate limit | found, D-26 |
| D, now 2 | where a resume lands | found, D-27 |
| D, now 3 | a `plan`-scoped in-process token for the human's seat | resolved, item 111 (the advisor and `ADVISOR_METHODS`) |
| D, now 4 | scouts and sightings in the live view | found, D-25 |
| D, now 5 | automatic saving | found, S6-07 |
| D, later | the "why" note's wording | found, S6-13 |
| D, later | the other twelve "later" entries | each repeats an A2 to A4 item above, or S1-12, S5-13, S5-14 or S6-38 |

---

## Appendix B: the lines that are not rows

So the header's arithmetic can be reproduced line by line.

- **Enum hits (5):** `proto/gp/api/v1/gateway.proto` `Applicability.HAS_PLACEHOLDERS`;
  `crates/verifier/src/structure.rs` `on_death_stub` doc; `tests/golden/verifier/`
  `e0101_missing_block`, `e0302_wait_without_timeout`, `e0303_hold_without_duration`
  `expected.report.json`.
- **Rule text (10):** `AGENTS.md` the status paragraph and section 12 (two); `CLAUDE.md` "Before you
  touch anything" (two) and "When you finish"; `.claude/workflows/wave-lanes.js` the lane prompts
  (four).
- **References (27):** `crates/client-gdext/src/pacer.rs` test `a_speed_outside_the_set_is_refused`;
  `crates/client-gdext/src/rig.rs` `SEAT_CALLS_PER_REFILL` (naming `CALLS_PER_TICK`);
  `crates/client-gdext/tests/godot_project.rs` `the_export_ships_the_smoke_check_and_no_other_test_file`;
  `crates/gamectl/src/scenario/run.rs` `open`; `crates/gateway/src/surface.rs` `set_limits`;
  `crates/gateway/src/token.rs` `with_lifetime` and test `the_default_lifetime_is_the_whole_match`;
  `crates/operator/src/compose.rs` `why_safe`; `crates/operator/src/easy.rs` module doc;
  `crates/sim/src/mapgen.rs` module doc (two); `crates/sim/src/world.rs` module doc and
  `respawn_curve`; `crates/sim/tests/determinism.rs` `the_committed_rules_table_carries_the_decided_values`;
  `crates/sim/tests/economy.rs` `a_dormant_beacon_powers_down_everything_homed_to_it`;
  `crates/sim/tests/mapgen.rs` `supply_is_within_the_power_ceiling`; `godot/README.md` "The editor
  (T19, pull request 2)" and "The export (T21)"; `godot/scripts/lobby.gd` `_build_ui`;
  `packaging/README.md` the heading "## PLACEHOLDERs"; `proto/gp/api/v1/gateway.proto` file header;
  `proto/gp/v1/playbook.proto` file header; `rules/rules.v1.json` and
  `tests/golden/proto/expected.rules.v1.json` `note`; `tests/golden/package/README.md` "What it
  pins" and `tests/golden/plan-core/README.md` "What a diff means" (both stale; T22a rewrites them);
  `tests/golden/vista/README.md` "What the golden shows".
- **Tombstones (11):** `crates/client-gdext/src/editor.rs` `carry_forward` (pull request 1's);
  `crates/gateway/src/surface.rs` `plans_for_round`; `crates/gateway/src/surface/planning.rs`
  `render_plan`; `crates/mesher/src/lib.rs` `voxel_index`; `crates/plan-core/src/context.rs`
  `segment_length_ms`; `crates/sim/src/runner.rs` module doc; `crates/sim/src/world.rs`
  `sealed_units`; `crates/sim/tests/determinism.rs`
  `the_asset_id_space_widened_without_renumbering_a_unit`; `crates/sim/tests/pathing.rs`
  `a_sealed_in_walker_parks_and_is_re_armed_when_the_graph_changes`; `proto/buf.gen.yaml` the codec
  note; `proto/gp/api/v1/gateway.proto` file header (the casing PLACEHOLDER item 80 resolved).
- **Continuations (16):** `crates/gateway/src/feed.rs` `Digest`; `crates/plan-core/src/strings.rs`
  module doc (two); `crates/proto/src/fingerprint.rs` module doc; `crates/sim/src/lib.rs`
  `DETERMINISM_SEGMENT_LENGTHS_MS`; `crates/sim/src/world.rs` `ore_in_sphere`,
  `draw_new_destinations`, `fill_unit_table`; `crates/verifier/src/strings.rs` module doc;
  `crates/verifier/src/structure.rs` `walk_action`; `godot/scripts/camera_rig.gd` `PAN_RATE`;
  `godot/scripts/editor.gd`, `rows.gd`, `strings.gd` and `vista.gd` headers;
  `tests/golden/mapgen/README.md` "The format".
- **Pointers (23):** `crates/client-gdext/src/editor.rs` `preview_place`; `crates/gateway/src/frame.rs`
  `MAX_FRAME_BYTES`; `crates/gateway/src/limit.rs` `CALLS_PER_WINDOW`, `WINDOW_TICKS`;
  `crates/gateway/src/surface/knowledge.rs` `what_if_budget`; `crates/mesher/src/sweep.rs`
  `LIGHT_SPAN_256`; `crates/operator/src/safe.rs` `power_short`, `at_risk`;
  `crates/sim/src/mapgen.rs` `TERRAIN_COARSE_AMPLITUDE`, `TERRAIN_FINE_SHIFT`,
  `TERRAIN_FINE_AMPLITUDE`, `TERRAIN_SKIN_VOXELS`; `crates/sim/src/power.rs` `draw_of`;
  `crates/sim/src/programs.rs` the Survey program's `run`; `crates/sim/tests/economy.rs`
  `a_beacon_whose_shed_relieves_nothing_is_still_shed_ahead_of_the_core`;
  `godot/scripts/camera_rig.gd` `ZOOM_STEP`, `TURN_RATE`, `FOLLOW_RATE`, `NEAREST`, `yaw`, `pitch`,
  `distance`; `proto/gp/v1/seams.proto` `BuiltinProgram.program_id`.

---

## Re-running the register

At each stage's close (AGENTS.md section 10 item 7), run the header's command at the closing base,
compare its count with this header's 588, and walk the difference: a line that is new is a new row
or a copy of an old one; a line that is gone is a row resolved, which leaves this file with its
item. Re-read "Stale" and "Malformed" against the stage that just closed, since a stage's close is
what makes an entry stale. Until S1's contract pull request brings a strict one-line grammar and an
`xtask` check (S1-10), the register is kept by hand this way, and the grep's count moves on every
comment edit.
