<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Stage demo run sheet: S2, destruction and combat

The owner's review of the stage demo ends S2 (AGENTS.md section 10 item 8). This sheet walks the
demo of `docs/design/s2-plan.md` section 1.1 as the zip plays it, one step at a time: what to do,
what to see, and which test or scenario also proves the step, or "by hand only" where none does.

It is written against `main` at `b7d2b0b` (`rush` #155, the last S2 lane before this sheet). The
words in quotation marks are the product's own: the gateway's feed lines from
`crates/gateway/src/strings.rs`, the verifier's from its diagnostic catalogue, the lobby's from
`godot/scripts/`. A number in angle brackets, such as `<N>`, is one the match decides and this sheet
cannot know. Row IDs such as S2-17 are rows of the PLACEHOLDER register, `docs/placeholders.md`.

**How the steps were checked.** The main session cannot watch the vista, so every match step below
was played headless first, on the zip's own seed and seats, with the same playbook the sheet asks
you to load: a scenario file that is not committed, seat 0 sealing that playbook every round and
seat 1 played by Easy (`"kind": "builtin"`), six rounds on the 3 / 5 / 8 ladder, with one
`event_fired` assertion per event kind, seat and round end. "Headless, round N" below means that
probe saw the event by the end of round N's Push. It sees event kinds and seats, not which structure
or which beacon, so where a step names one, the name is the sheet's reading and the check is yours.

**What this demo cannot show, said once here.** Section 1.1 was written expecting Easy to attack.
**Easy never attacks.** Under fog it sights nothing it could raid, and it cannot expand after round
1, so in every round after the first it seals its safe playbook. `tune2`'s balance check played 220
headless matches and no seat dealt another any damage (#153); the probe above saw no
`attack_launched` and no `enemy_beacon_found` for seat 1 in six rounds. So any fighting in this
match comes from your raids: your scouts and raiders die at Easy's core, and your raid razes one of
Easy's beacons. Your mortar, your wall and your repair drones never meet an enemy here. Section
1.1's steps 3, 4, 6 and the collapse are shown beside the zip instead, headless, and the sheet says
which is which. "Deviations from section 1.1" lists each difference.

## Before you start

**The zip.** No S2 tag was cut. The demo uses a package of `main` at `b7d2b0b` built locally:

- Windows: `D:/build/s2-demo/pharmakos-s2-b7d2b0b-windows-x86_64.zip`, 117 916 857 bytes, 27
  entries, SHA-256 `40811dd853408b43d29b7a04add635780610c4ef6c486ec3049bd1fda81ec170` (also in
  `D:/build/s2-demo/SHA256SUMS.txt`). `cargo xtask package` built it; its in-editor smoke check
  printed "[smoke] OK", and it replayed every committed scenario with the packaged `gamectl` against
  its golden chain, the eight under `scenarios/s2/` included, all "ok".
- Linux: the `pharmakos-linux` artifact of CI's `package (ubuntu-24.04)` job in `main`'s push run
  for `b7d2b0b`, run 40745227332.

The zip holds `Pharmakos.exe`, `gamectl.exe`, the client library, `library/` (five templates) and
`rules/`. It holds no scenario files.

**Unzipping and folders.** As at S1: extract the whole zip into a folder of its own and run
`Pharmakos\Pharmakos.exe` (unsigned: "More info", then "Run anyway" if SmartScreen asks). The match
cache is under `%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`, the lobby's `last_match.txt` and the
log under `%APPDATA%\Godot\app_userdata\Pharmakos\`.

**Pressing Ready.** Ready ends a Lull at once: Easy is always ready. **Ready with nothing submitted
files the safe playbook for you**, so in every Lull press "Submit" first and wait for "Submitted.
This is now your sealed order." before you press "Ready". The editor carries last round's text into
the next Lull; it still has to be submitted again.

**Have ready:**

- A checkout of `main` at `b7d2b0b` (the owner's clone is enough) and a Rust toolchain, for
  "Beside the demo".
- The playbook below, saved as `s2-demo.jsonc` in a folder of playbooks. Its first step is `site`'s
  committed base-kit visit (`scenarios/s2/base-kit-at-the-core.playbook.jsonc`) with a mortar
  (`terraform-levels-a-footprint`'s anchor), `terraform: BOTH` and `roe: FREE_FIRE` added on the
  core. Then come the nightly Rusher's three placing steps and its closing move
  (`scenarios/nightly/rusher.playbook.jsonc`, `rush` #155), with its `allow_dormant_beacons` option
  and its `on_death`: a Survey beacon with two scouts over the map's eight possible spawn points,
  an Attack beacon that raids the nearest enemy beacon you know of (tagged `raid` here, `rush`
  there), a Generator if power runs short, and a walk home. Left out: the Rusher's opening move to
  safety, and base-kit's `max_deaths_before_fallback`.

```jsonc
// The S2 demo's playbook, seat 0 on the golden seed.
{
  "schema_version": {"major": 1, "minor": 0},
  "meta": {"title": "S2 demo", "author_kind": "HUMAN"},
  "kind": "PLAYBOOK",
  "declarative": {
    "route": [
      {"label": "arm",
        "interface": {"beacon": {"beacon_id": "b_00"}, "rows": [
          {"set_mandate_settings": {"roe": "FREE_FIRE", "build": {
            "targets": [
              {"blueprint_id": "autocannon", "anchor": {"voxel": {"x": 362, "y": 30, "z": 36}}},
              {"blueprint_id": "wall", "anchor": {"voxel": {"x": 358, "y": 32, "z": 36}},
                "wall_box": {"min": {"x": 357, "y": 32, "z": 35}, "max": {"x": 360, "y": 32, "z": 37}},
                "order": 1},
              {"blueprint_id": "mortar", "anchor": {"voxel": {"x": 354, "y": 30, "z": 38}}, "order": 2}
            ],
            "repair_threshold_pct": 50,
            "rebuild_destroyed": true,
            "terraform": "BOTH"
          }}}
        ]},
        "timeout_ms": 60000, "on_fail": {"action": "SKIP"}},
      {"label": "look",
        "skip_if": {"beacon_hp_pct": {"beacon": {"nearest": {"filter": {"mandate": "SURVEY"}}}, "pct": {"op": "GE", "value": 0}}},
        "place_beacon": {"at": {"covering": {"seam": {"rank": "NEAREST", "coverage": "ANY"}}}, "tags": ["look"],
          "initial": {"mandate": {"survey": {
            "probe_areas": [
              {"min": {"x": 40, "y": 40, "z": 0}, "max": {"x": 40, "y": 40, "z": 63}},
              {"min": {"x": 192, "y": 40, "z": 0}, "max": {"x": 192, "y": 40, "z": 63}},
              {"min": {"x": 343, "y": 40, "z": 0}, "max": {"x": 343, "y": 40, "z": 63}},
              {"min": {"x": 40, "y": 192, "z": 0}, "max": {"x": 40, "y": 192, "z": 63}},
              {"min": {"x": 343, "y": 192, "z": 0}, "max": {"x": 343, "y": 192, "z": 63}},
              {"min": {"x": 40, "y": 343, "z": 0}, "max": {"x": 40, "y": 343, "z": 63}},
              {"min": {"x": 192, "y": 343, "z": 0}, "max": {"x": 192, "y": 343, "z": 63}},
              {"min": {"x": 343, "y": 343, "z": 0}, "max": {"x": 343, "y": 343, "z": 63}}
            ],
            "scout_count": 2}}}},
        "timeout_ms": 60000, "on_fail": {"action": "SKIP"}},
      {"label": "raid",
        "skip_if": {"beacon_hp_pct": {"beacon": {"nearest": {"filter": {"mandate": "ATTACK"}}}, "pct": {"op": "GE", "value": 0}}},
        "place_beacon": {"at": {"covering": {"seam": {"rank": "NEAREST", "coverage": "ANY"}}}, "tags": ["raid"],
          "initial": {"priority": "HIGH", "mandate": {"roe": "FREE_FIRE", "attack": {
            "target": {"beacon": {"nearest": {"filter": {"side": "ENEMY_KNOWN"}}}},
            "retarget": "NEAREST_KNOWN",
            "staging": {"x": 192, "y": 192, "z": 0},
            "launch": {"force_size": 2},
            "after_action": "REGROUP_AT_STAGING",
            "breach_allowed": true,
            "waves": 1}}}},
        "timeout_ms": 60000, "on_fail": {"action": "SKIP"}},
      {"label": "power",
        "skip_if": {"kw_headroom": {"kw": {"op": "GE", "value": 4}}},
        "place_beacon": {"at": {"covering": {"vent": {"rank": "NEAREST", "coverage": "UNCOVERED"}}},
          "initial": {"mandate": {"build": {"targets": [
            {"blueprint_id": "generator", "anchor": {"on": {"covered": {}}}}
          ]}}}},
        "timeout_ms": 90000, "on_fail": {"action": "SKIP"}},
      {"label": "home", "move": {"to": {"safest": {}}}}
    ],
    "handlers": [],
    "options": {"allow_dormant_beacons": true}
  },
  "on_death": {"on_respawn": "CONTINUE"},
  "fallback": {"hold": {"at": {"safest": {}}}}
}
```

- About ninety minutes. The first Lull runs 10:00 and the later ones 5:00, but Ready ends each at
  once; the Pushes run 3, 5, 8, 8, 8 and 8 minutes at 1×, which "4x" and "Skip" shorten.

## The lobby

1. **Launch the game with six rounds.** From a terminal in the unzipped folder run
   `Pharmakos.exe -- --rounds 6`, then press "New match".
   - See: the chooser as at S1; then "Round 1 - LULL  10:00". The match is two seats on the golden
     seed `0x00000000ca5caded`, you at seat 0 and Easy at seat 1, six rounds on the rules' 3 / 5 /
     8-minute ladder, rounds 4 to 6 taking its last entry. `--rounds` is a test-only launch option
     the lobby does not show (`godot/scripts/host_link.gd` `ROUNDS_ARG`; the default is
     `ROUND_LIMIT` 3).
   - Proof: `godot/scripts/watch_check.gd` `_rounds_cases` (the client's reading of the option, in
     CI's `client extension` jobs); `crates/client-gdext/tests/godot_project.rs`
     `the_lobby_hosts_a_three_round_match`. Six rounds on screen: by hand only.

## Round 1's Lull: the templates, then the demo playbook

2. **The wizard's five templates.** Under "Start from a template": "Defend the Core", "Expand &
   Mine", "Hold & Build", "Raid an Enemy Beacon" and "Safe playbook" (`library/*.jsonc` `title`).
   Open "Defend the Core" and "Raid an Enemy Beacon" in turn, read each, and press "Undo".
   - Defend the Core places a Defend beacon at the core on `FREE_FIRE` whose guards escort the
     commander inside its sphere (`oprd` #144). Raid an Enemy Beacon places an Attack beacon whose
     target is the nearest enemy beacon you know of (`atk2` #154). In round 1 you know none, so its
     check shows W0709 ("the seat has no sighting of an enemy beacon ... the force waits until there
     is one") and it still qualifies. Sealed alone it fields no scout, so it may never learn of a
     beacon: the nightly Rusher, aimed the same way with no scout, launched on 0 of 20 seeds
     (`rush` #155's probe table, "main (no scout) 0 / 20").
   - Proof: `crates/gamectl/tests/wizard.rs`
     `the_wizards_live_suggestion_qualifies_and_is_goldened_for_every_template` and
     `tests/golden/operator/wizard/{defend_the_core,raid_an_enemy_beacon}/`;
     `tests/golden/gateway/demo_raid_an_enemy_beacon_verify/expected.response.json` (W0709).

3. **Load the demo playbook, with `FREE_FIRE` on the Build beacon.** Press "Load..." and choose
   `s2-demo.jsonc`. Its first step walks the commander to the core, `b_00`, which is a Build beacon,
   and writes its settings: an autocannon, a wall box, a mortar, a repair threshold of 50, rebuild
   on, `terraform: BOTH`, and `roe: FREE_FIRE`. Without that `roe` the core reads `RETURN_FIRE`
   (decision 4; I0003 says so on an omitted `roe`), and a mortar on return fire waits to be hit
   before it shells anything.
   - See under "Checks": no error; the probe below sealed this file through `submit_plan` on this
     seed. `gamectl verify` against its own reference view (not the match's snapshot, which the
     editor checks against) answers "qualifies no (1 error)", E0401, because its reference view
     has no `b_00`, and also I0003 on the Survey and Generator beacons' omitted `roe` and I0004.
     Its warnings are the ones you may see here: W0709 on the raid's target (as in step 2),
     W0602 that the playbook's committed spending outruns this round's treasury (`gamectl verify`
     against its reference view says "committed spending of $ 457 outruns the projected treasury of
     $ 200"; the Build targets wait for money, round by round), and W0704 or W0705 on the seam and
     vent descriptions. **No W0501** in any case: `dfnd` #140 retired it.
   - See under the projection: the wall priced per voxel at `structures.wall_cost_per_voxel`, $ 1,
     by its box (`crates/verifier/src/projection.rs`'s table, "its box's voxels, at most"). The
     figure on screen: by hand only.
   - Proof: the headless probe sealed this file for seat 0 through `submit_plan` every round
     (`plan_sealed` and `row_committed` for seat 0 in round 1, the first from a second probe run
     with the same playbook); `scenarios/s2/base-kit-at-the-core`
     and `scenarios/s2/terraform-levels-a-footprint` play the autocannon, the wall and the mortar at
     these anchors, and the package replayed both, "ok".

4. **Submit, then Ready.** "Submitted. This is now your sealed order.", then "Ready".

## Round 1's Push: the base kit

5. **The core arms itself.** Watch at 1× or 2×.
   - See: the commander walks to the core and the row commits; the core's two build drones raise
     the autocannon and fill the wall from the ground up, two voxels high at least
     (`materials.wall_min_height_voxels`); with `terraform: BOTH` the ground under the mortar's
     footprint rises or falls to the anchor's height before it builds. "A structure of seat 0 is
     finished." in the feed. The Survey beacon goes down on the nearest seam and fabricates its two
     scouts.
   - Headless, round 1: `row_committed`, `structure_queued`, `structure_completed`,
     `beacon_placed` and `unit_fabricated` for seat 0; also `step_failed`, `step_skipped` and
     `fallback_engaged` for seat 0 (and `fallback_engaged` for Easy). Which step fails, and whether
     the fallback is the route running out rather than a failure, the probe cannot say: note the
     feed's step lines by hand. Whether the mortar is finished in round 1 or
     later, as the treasury allows: by hand only (note the round).
   - Proof: `base-kit-at-the-core` (`structure_completed` for seat 0 by tick 1200) and
     `terraform-levels-a-footprint` (`structure_queued` and `structure_completed` by tick 1200),
     each with its chain.

## Round 2: the scouts find Easy

6. **Lull:** the editor carries the text. Submit, then Ready.
7. **Push.** Your scouts walk to the spawn points.
   - See: "A Survey scout of seat 0 found an enemy beacon." and later "... refreshed a sighting
     <N> ms old." Some of your scouts die near Easy's core ("A unit of seat 0 was destroyed."), and
     Easy is credited for them; Easy's reclaim drones salvage what they leave ("A reclaim drone of
     seat 1 delivered salvage worth $ <N>."), at 25 % of the build cost (`economy.salvage_percent`).
   - Headless, round 2: `enemy_beacon_found`, `sighting_refreshed` and `unit_destroyed` for seat
     0; `kill_credited` and `salvage_delivered` for seat 1. No `attack_launched` for either seat.
   - Proof: `scenarios/s2/survey-finds-the-enemy` (`area_surveyed` and `enemy_beacon_found` for
     seat 0 by tick 6000); `crates/sim/tests/site.rs` `a_mortar_duel_repairs_and_salvages`.

## Round 3: the raid

8. **Lull:** Submit, then Ready. W0709 goes while your seat holds a sighting of an enemy beacon
   inside `verifier.reach_memory_ms` (180 000 ms); if it stays, the force still launches once one is
   fresh at launch time, since the target is chosen then.
9. **Push.** The Attack beacon's raiders gather at the staging point (the map's centre) and launch
   on a force of two at the nearest enemy beacon you know of, measured straight-line from the
   staging point (decisions-log item 151 (2) (a)).
   - See: "An Attack beacon of seat 0 launched its force of <N>."; your raiders kill Easy's assets
     ("seat 0 was credited $ <N> of a destruction."); "A beacon of seat 1 was destroyed."; then "An
     Attack beacon of seat 0 ended its attack: <reason>." Easy's repair drones repair what it can.
   - Headless, round 3: `attack_launched`, `attack_ended` and `kill_credited` for seat 0;
     `unit_destroyed`, `structure_destroyed`, `beacon_destroyed` and `asset_repaired` for seat 1.
     The probe does not say which beacon fell. The match does not end, so Easy keeps a beacon.
   - Proof: `crates/sim/tests/attack.rs`
     `a_raid_on_the_nearest_known_beacon_launches_at_the_nearest_sighting` and
     `a_tie_between_two_equally_near_known_beacons_goes_to_the_lowest_id`;
     `scenarios/s2/breach-and-raze` (`attack_launched`, `charge_planted`, `charge_detonated` and
     `beacon_destroyed` for seat 1 by tick 6400).
10. **A beacon falls (section 1.1 step 7).** Where Easy's beacon stood, look for its structures
    turned to neutral ruins and its walls to neutral walls; the client still draws them in Easy's
    colour (E14). Its units retreat and re-home to Easy's nearest beacon.
    - Headless: `structure_ruined` carries no seat, so the probe's per-seat assertions cannot see
      it, and it is missing from the kinds the probe reports as fired; the beacon that fell here may
      have held no structure. By hand only.
    - Proof: `crates/sim/tests/runner.rs` `a_beacon_death_ruins_the_structures_homed_to_it`;
      `crates/sim/tests/combat.rs` `a_ruin_and_a_disbanded_unit_credit_nobody`.

## Rounds 4 to 6: by hand, then the end

11. **Optional: an enemy beacon by handle.** In round 4's Lull, under "What the map reads now", note
    the handle of one of Easy's beacons your seat has sighted (`e_<NN>`, as `e_01`). Add a step that places a
    second Attack beacon with `"target": {"beacon": {"beacon_id": "e_<NN>"}}`, `"retarget":
    "NONE"` and no `skip_if`, copying the raid step's other fields. Submit, then Ready.
    - See: the check names no E0418 while the sighting is fresh; the target is pinned at the seal.
      Not played headless: a scenario seals one playbook for every round, and a handle is known only
      after round 2. By hand only.
    - Proof: `crates/sim/tests/attack.rs` `the_target_is_pinned_at_the_seal`;
      `tests/golden/verifier/e0418_an_attack_target_seen_too_long_ago/` for the refusal of a stale
      target.
12. **Optional: hunt the seatless enemy commander.** As in step 11, add a new placing step with
    no `skip_if` (the raid step's `skip_if` skips it once an Attack beacon stands), with the target
    `{"enemy_commander": {}}`. The check shows W0708 when no commander sighting is fresh, and the
    file qualifies.
    - Headless: the nightly Hunter, played on this seed at seat 0, placed its Attack beacon and never
      launched in six rounds: its scouts never sighted Easy's commander, which stays home. So a hunt
      here waits, counted, and that is the build's behaviour, not a fault.
    - Proof: `scenarios/s2/hunt-the-commander` (`attack_launched` by tick 6300 and `commander_died`
      for seat 1 by tick 7500, on a staged sighting); `crates/sim/tests/attack.rs`
      `a_hunted_commander_is_retargeted_while_seen_and_lost_when_not`;
      `tests/golden/verifier/w0708_a_seatless_hunt_with_no_sighting/`.
13. **The recap, between rounds.** It shows your own tally only: "This match so far you destroyed
    <N> enemy assets, worth $ <N>, and lost <N> assets." (a shared kill is named when one helped).
    Easy's tally is on no wire until the match ends (decisions-log item 145 (7)).
    - Proof: `crates/gateway/src/strings.rs` `recap_tally`; `crates/gateway/src/surface.rs`
      `a_recap_between_rounds_carries_no_other_seats_tally`, `the_recap_names_a_shared_kill` and
      `the_recap_at_match_end_carries_every_seats_tally`.
14. **The end.** The match ends at round 6's limit (the probe played all 48 000 Push ticks), unless
    Easy loses its last beacon first (`last_seat_standing`). The final audit names the winner; the
    recap shows every seat's tally.
    - Proof: `crates/sim/tests/attack.rs` `the_last_beacon_lost_ends_the_match_that_tick`. The
      winner on screen: by hand only.

## Beside the demo

Run these from the checkout at `b7d2b0b`, with your own target directory. Each was run for this
sheet on the owner's i7-9800X; the outputs quoted are from those runs.

### Decision 39's collapse, staged

**This beat is staged, and not played.** No geometry the game can reach puts an asset on ground a
blast can detach: structures and beacons stand on footings of anchored ground (the owner's ruling,
item 148 (2)), so a shelf carrying one cannot fall, and no playbook reaches a shelf with units on it
under the owner's mortars at CI's cost (decision 39). `fall` #132's `collapse_credits_the_blast`,
re-staged by `site` #133 with units, a wreck and a ruin, builds the shelf through test seams and
orders the two mortar shells itself:

```sh
cargo test --release -p pharmakos-sim --test support -- --exact collapse_credits_the_blast
```

printed `test collapse_credits_the_blast ... ok` for this sheet. The test prints no event log of its own. To show
one, a local copy of the test with one added line (`for event in &feed { eprintln!("{event:?}"); }`
after its first run, not committed) printed the run's fifteen events; positions are in voxels below
(the log's `Fx` values divided by 65 536):

| Tick | Event | Seat | Value |
|---|---|---|---|
| 239 | `MatchStarted`, `LullOpened`, `PushStarted` | — | — |
| 240 | `BeaconDestroyed` (the staging destroys a seat 1 beacon, so its structure is a ruin) | 1 | 0 |
| 240 | `StructureRuined` | — | 0 |
| 348 | `ShellLanded` at (35, 361, 28) | 0 | 35 |
| 348 | `ShellLanded` at (35, 355, 28) | 0 | 35 |
| 348 | `RegionCollapsed` at (33, 356, 26) | — | 17 |
| 348 | `AssetFell`: seat 1's commander, seat 1's drone, the ruin, the wreck | 1, 1, —, — | 100, 100, 100, 0 |
| 348 | `KillCredited` for the drone | 0 | 20 |
| 348 | `UnitDestroyed`: the drone | 1 | 2 |
| 348 | `WreckLeft` | — | 5 |

The two craters cut the shelf's last support on tick 348 (`CUT_TICK`, 109 ticks into the Push); the
shelf falls that tick, everything on it takes `collapse.fall_damage_hp` (100) once, the drone (100
HP) dies, and its kill is seat 0's, the seat whose craters detached it. The commander's fall
credits nobody, nor does the ruin's (spec §4). The test runs the staging three times, once through a
save and restore at tick 60, and pins the chain at ticks 108, 109 and 110.

### Section 1.1's combat beats Easy cannot bring

Section 1.1's steps 3, 4 and 6 need an enemy who comes to you. They are shown headless:

- **Repair, salvage and crater refill under a mortar duel** (steps 3 and 4):
  `cargo test --release -p pharmakos-sim --test site -- a_mortar_duel` printed `test
  a_mortar_duel_repairs_and_salvages ... ok` and `test a_mortar_duel_refills_its_craters ... ok`
  for this sheet.
- **The committed S2 scenarios**, each `gamectl scenario run scenarios/s2/<name>.scenario.jsonc`
  (`rules_hash 55019fca46a1a843`, every assertion "ok", its chain included): `raid-on-a-mortar`
  (`attack_launched` for seat 0 by tick 800, `unit_destroyed` for seat 0 by tick 6100,
  `wreck_left` by tick 6100), `mortar-breaks-wall` (`shell_landed` for seat 0 by tick 5700),
  `breach-and-raze` (`charge_planted` by tick 5900, `charge_detonated` by tick 6000,
  `beacon_destroyed` for seat 1 by tick 6400), `hunt-the-commander`, `guards-escort-the-commander`,
  `survey-finds-the-enemy`, `base-kit-at-the-core` and `terraform-levels-a-footprint`.
- **Step 6, the commander hunted, wounded, healed and respawned**: `crates/sim/tests/defend.rs`
  `a_wounded_commander_is_walked_to_a_repair_drone`; `crates/sim/tests/site.rs`
  `a_repair_drone_heals_the_commander`; `crates/sim/tests/runner.rs`
  `the_respawn_delay_grows_with_each_death_in_the_same_push`.

### G3′-real, re-certified on `tune2`'s numbers

S2's plan, section 5 item 5: `tune2` #153 moved the rules the gate load reads, so the certification
was re-run on them at `b7d2b0b`, on the owner's i7-9800X, three repetitions per OS, each alone with
nothing else building, by `g3r`'s command:

```sh
cargo test --release --locked -p pharmakos-bench --test g3_real -- --ignored --nocapture \
    --exact g3_real_segment_support_tick_and_fourth_seat
```

Windows natively, and WSL2 Ubuntu through `C:/Users/PC/ClaudeProject/bin/wsl-ci.sh <repo> --
<command>` (verdict `wsl-ci: PASS rc=0 sha=b7d2b0b323a1`). Every run printed seed
`0x00000000ca5caded`, bench `rules_hash ed91bae0206344cf` (the committed table's
`55019fca46a1a843` with the bench's `power.core_surplus_kw` 10 → 133 and `power.map_ceiling_kw` 190
→ 559) and layout `46c4018ed990aa15`.

| i7-9800X, `b7d2b0b` | rep 1 | rep 2 | rep 3 | **median** | bar |
|---|---|---|---|---|---|
| Windows: tick p99 | 11.630 ms | 11.529 ms | 11.665 ms | **11.630 ms** | G3′-a ≤ 25 ms |
| Windows: tick mean | 4.916 ms | 4.919 ms | 4.910 ms | **4.916 ms (10.17×)** | G3′-b ≤ 12.5 ms (4×) |
| WSL2 Linux: tick p99 | 12.419 ms | 12.503 ms | 12.429 ms | **12.429 ms** | ≤ 25 ms |
| WSL2 Linux: tick mean | 5.159 ms | 5.168 ms | 5.168 ms | **5.168 ms (9.67×)** | ≤ 12.5 ms |

**Certified on `tune2`'s rules: tick p99 12.43 ms and mean 5.17 ms (WSL2 Linux, the slower OS).**
The gate passes with about half its p99 to spare; no fallback is taken. Against `g3r`'s own
certification (13.05 ms and 5.34 ms at `da99290`) both figures fell a little.

- **Clocks.** Windows' `Win32_Processor.CurrentClockSpeed` read 3 792 MHz before and after every
  repetition (logged beside the runs, `D:/build/demo2/g3-win-clocks.txt`), and WSL's `/proc/cpuinfo` 3 791.998 MHz over 8 CPUs; both are nominal and carry no
  thermal signal, as `g3r` found. The per-minute p99 falls from minute 1 to minute 8 on both OSes
  (Linux repetition 3: 13.1 → 10.3 ms), so no drift.
- **What moved under the load.** Repaths served fell from `g3r`'s 118 767 to 1 356 over the 9 600
  ticks (9 ticks spent the whole cap of 16, the largest backlog 20), since `tune2` removed the
  harness walkers' random wander (S2-16): pathing is still most of the tick (Windows mean 4.18 ms),
  but the repath cap is now barely exercised by the gate load. 143 deaths and 141 kill credits over
  the segment; 46.3 units walking a tick, 232 alive at the end. The support search's adversarial
  tick took 3.57 ms on Linux (3.48 on Windows), 4 915 of its 16 384 visits. A fourth, unplaced seat's
  line is noise around 0, as item 149 (3) (e) found: Linux -128, 669 and 150 ns, Windows 90, -83
  and -840 ns.
- **What the load still leaves out** (`g3r`'s question 1, unchanged): the layout seam carries no
  mandate settings, so Attack never launches in the gate load, and a launched force's walk is not in
  the measured tick.

### The nightly report

Forced on `main`: `gh workflow run nightly-scenarios.yml --ref main -f force=true`, run
40745425651 on `b7d2b0b`, every job green: the three arms, the verifier fuzz (10 000 playbooks),
G4's chains on windows, ubuntu and macos and their cross-OS compare. Each arm's log line reads "20
seed(s) played, the arm the sole winner of <N>":

| Arm | Sole wins vs Easy | Band (an alarm above 65 %, more than 13 of 20) | Acknowledged |
|---|---|---|---|
| Rusher | 8 of 20 (40 %) | within | as Easy's |
| Turtle | 0 of 20 | within; a floor, since it builds no walls (S2-17) | as Easy's |
| Hunter | 2 of 20 (10 %) | within | as Easy's |

These are `rush` #155's figures on its own forced run (40729956058), unchanged. **Each band is
acknowledged as Easy's** (item 144 (6)): Easy never attacks, so these bands measure the arms against
an opponent that never comes for them, and they are re-baselined at S5 when Easy's operators grow.
Nothing checks that the Rusher launches on every seed (`rush`'s probe found 20 of 20 in round 2);
that guard is owed (decisions-log item 151 (4)). `NIGHTLY_SCENARIOS_ENABLED` is still unset; setting
it is the owner's (decision 30).

### The balance and duel reports

`tune2` #153 attached them; the owner tunes combat's numbers from them (item 144 (8)). Its PR body
holds both. In short: no seat damaged another in any of the balance check's 220 matches, so the
three-seat kingmaking figure is empty; the duel report holds spec §9's counter relations (the
autocannon kills a raider in 2.2 s by the rows; four raiders kill an autocannon in 10 s; the mortar
kills an autocannon 30 voxels off in 40 s with a spotter and never fires without one).

## Deviations from section 1.1

1. **Easy never attacks** (steps 3, 4 and 6): no raiders come, nothing reaches the mortar, the
   mortar never fires in the match (the probe saw no `shell_landed` for seat 0), and the owner's
   commander is never hunted. Shown headless instead ("Beside the demo").
2. **Hold & Build is not what is sealed** (step 1): Hold & Build places a new Build beacon on a vent
   and writes no `roe`; the sheet arms the core, `b_00`, a Build beacon, with `FREE_FIRE` instead,
   because a mortar at the core is the one Easy's base would meet first and is where `site`'s
   committed anchors are proven.
3. **The breach** (step 5): Easy builds no wall, so no breach happens in the match; the raid's
   `breach_allowed` is set and unused. Shown by `mortar-breaks-wall` and `breach-and-raze`.
4. **The owner's raid comes in round 3, not round 4** (step 5), and targets the nearest known enemy
   beacon, as decisions-log item 150 (2)'s `atk2` built; a handle and the commander are optional by
   hand (steps 11 and 12).
5. **The collapse is staged** (step 5), as section 1.1 says.
6. **The match ends at round 6's limit** in the headless probe, not on `last_seat_standing`.

## Owner rulings at the demo

Each comes with the recommendation of the lane that raised it. The main session's calls on the
merges are decisions-log item 151 (2), each open to veto.

1. **The S2 tuning rows** (`rules/rules.v1.json` revision 6, 60 `rules.proto` markers "at S2's
   demo"/"S2"/"at S2's exit"; register "Tuning, delegated"). Recommendation: keep revision 6 and
   move every row to S3, re-read when Easy fights.
2. **The non-rules markers at S2's demo** (register, "At S2's close"): the empty Attack and Defend
   arms' readings, `BREACH_COLUMN_COST` 100, Survey's 16-column cell and 60 s refresh, the fielding
   totals 300 / 40 / 120, the site top-up's five seconds, the commander's speed, who the audit
   decides between, the balance check's seeds and flags, and the repair threshold of 50.
   Recommendation: keep each; close the fielding totals' row (S2-08), which `g3r` measured, and
   move the rest to S3.
3. **The Turtle's walls** (S2-17). Recommendation: move to S5, with the bands' re-baseline.
4. **`atk`'s `e_NN` pin across a resume:** from round 2 a resumed match could pin a handle to a
   different beacon than the unbroken run. Recommendation: a contract fix in S3 (first-sighting
   order in the sim, or the handle table in the save).
5. **`atk`'s other questions:** the charge's cut on slopes (a cliff at the rim; decision 40 assumes
   flat ground) and its written readings (`HOLD_FIRE` never launches; the weak point counts finished
   structures only; a lost charge is re-bought without limit). Recommendation: accept as built.
6. **`surv`'s four:** the forget rule (keep), re-scanning (decide at S3), remembered armed sightings
   as known threats (yes, as a later ruling), the cell against scout vision (sized by `tune2`).
7. **`oprd`'s:** a resumed Lull forgets a commander death, so Easy adds no guard where an unbroken
   match would (accept the gap), and its edits outside its named places (accept).
8. **Item 149 (4)'s `dfnd` questions:** an empty Defend arm's readings, and a damaged guard's
   unbounded walk to a repair drone; and #139's fourth-seat line and #140's `NEVER` near the
   commander, which stand as built.
9. **The Rusher's marker, discharged by `rush` #155** ("how the Rusher comes to know an enemy
   beacon", which named the owner): the main session accepted the Survey-first rush (item 151 (2)
   (g)). Recommendation: confirm.
10. **`g3r`'s two questions.** (1) The gate load still has no launched Attack force, because the
    layout seam carries no mandate settings; the seam was not sequenced before this
    re-certification. Recommendation: accept the certification (p99 12.43 ms against 25) and give
    the seam to S3's first `crates/sim` lane. (2) `g3 budget (ubuntu-24.04)` is not required but
    can go red, and `scripts/merge-train.sh` stops on it. Recommendation: keep it able to fail (E18).
11. **Two owner-named markers the register moves to S3:** the delta mode for `get_view`'s entities
    (S2-12, `gateway.proto`) and the view's recompute-key cost (S1-34, `watch.rs`, "at S2's
    G3′-real"), neither measured in S2. Recommendation: confirm the move.
12. **`NIGHTLY_SCENARIOS_ENABLED`** (decision 30): `arms` has merged and the forced runs are
   attached. Recommendation: set it.
13. **Whether the merge delegation runs on into S3** (item 141).

## Found while writing this sheet

- `cargo xtask placeholders` reads `godot/fixtures/instantiate_suggested.json:5` as a marker whose
  when is "at S2's demo" followed by a JSON line break and `"initial"`: the fixture is a byte copy of
  a JSON golden whose string holds the marker. It parses, so the step stays green; the register
  counts it with the S2 markers.
- The headless probe's `structure_completed` cannot say which structure finished; a scenario
  assertion naming a structure kind would let a sheet prove step 5's mortar.
