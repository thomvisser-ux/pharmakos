<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Stage demo run sheet: S3, authoring

The owner's review of the stage demo ends S3 (AGENTS.md section 10 item 8). This sheet walks the demo
of `docs/design/s3-plan.md` section 1.1 as the zip plays it, one step at a time: what to do, what to
see, and which test or scenario also proves the step, or "by hand only" where none does. The main
session drives the zip by computer control while the owner watches (section 1.1), and presses Ready
to end a Lull.

It is written against `main` at `a8aa864a` (`a8aa864a88f2a7ef7129d36d381c994b58502c8e`, the merge of
#255), the demo's tree. #255 merged after #256 (`5e62ba7`, which recorded the certifying runs of item
223): in shipped code it changes the text of three messages, two internal errors in
`crates/gateway/src/serve.rs` (the control pipe failing) and one rules-load refusal in
`crates/sim/src/attack.rs` (`combat.charge_damage_hp` out of range), each of which had kept a stray line
break and indent; the rest of it is tests. Since that is a change to the sim and the gateway after
`7a604e22`, item 223's condition holds by its letter, so **the zip was rebuilt on `a8aa864a` and the
certifying runs were retaken on it by the main session**; their record is decisions-log item 225. The
main session re-reads this sheet after any later merge. The words in quotation marks are the product's own: the gateway's from
`crates/gateway/src/strings.rs` and `crates/plan-core/src/strings.rs`, the verifier's from its
catalogue, the lobby's and the editor's from `godot/scripts/strings.gd`. A number in angle brackets,
such as `<N>`, is one the match decides and this sheet cannot know. Row IDs such as S3-33 are rows of
the PLACEHOLDER register, `docs/placeholders.md`.

**The P6 gate, in one line:** `<filled by the main session from docs/demo/p6c-record.md: template
<t> s, scratch <s> s, typing clause <met or missed>>`, AI-driven by the main session, **not a person's
time**; the owner rules at this review whether S3 closes on it (section 5.3's closing rule).

**How the steps were checked.** The main session cannot watch the vista, so every match step below
was played headless first, on the zip's own seed (Probation's, 20) and seats (seat 0 the playbook,
seat 1 Easy, `"kind": "builtin"`), four rounds on the 3 / 5 / 8 ladder, in scenario files that are
not committed (under `D:/build/demo3/probe/`, played by the zip's own `gamectl` and by a scratch
event-log reader over `pharmakos_gamectl::scenario`, also not committed). Two forms were played: the
sheet's final text carried whole through all four rounds (the "whole-text probe": in rounds 1 to 3 it
holds rows that do not yet qualify, E0416 and E0507, so the runner seals it through its harness door
there), and each round's own text carried from round 1 (the "round probes": round 1's and round 2's
texts qualify through `submit_plan` in every round they were sealed). "Headless, round N" below means
a probe saw the event by the end of round N's Push. Every probe was played first on `5e62ba7`'s zip
and again on `a8aa864a`'s: each event log is identical on the two, tick for tick, as #255 moved no
golden. A probe sees event kinds, seats and ticks, not
which structure or beacon: where a step names one, the name is the sheet's reading and the check is
yours.

**What this demo cannot show, said once here.** Four beats of section 1.1 are not played live:

1. **Step 4's brownout.** No route to a brownout fits this beat's money on seed 20; the beat is shown
   staged, from S1's `brownout-by-recycle` scenario and the gateway's memo test ("Beside the demo").
2. **Easy's raid and the first death's memo (step 7).** On seed 20, with this sheet's playbook, Easy
   first sights your beacons in round 5 and launches no raid in eight rounds, so in a four-round match
   **Easy's raid is not shown live**, and your commander does not die. Both are shown staged.
3. **The Spire's destruction (step 9)**, staged from `lic`'s `spire-lost` scenario, as section 1.1
   says, since no scripted match is sure to destroy it.
4. **The money's margin** is not a beat, but the licence beat rests on it: it was measured headless,
   not on screen.

Decision 13 took its first option (the preset's own seed), so the licence beat, steps 4 to 8, is
played live.

## Before you start

**The zip.** No S3 tag is cut (decision 10 cuts `v0.1.0-rc.1` after this review; that is the owner's).
The demo uses a package of `main` at `a8aa864a` built locally by `cargo xtask package`, from a
detached checkout of that commit:

- Windows: `D:/build/demo3/pharmakos-s3-a8aa864a-windows-x86_64.zip`, 120 357 327 bytes, 27 entries,
  SHA-256 `12b47f3f1c220873ebcca2212f023927e6a8689a5e98ba15c4139321d74d2a08` (also in
  `D:/build/demo3/SHA256SUMS.txt`); its `README.txt` reads "Build: commit a8aa864a88f2". Its
  in-editor smoke check printed "[smoke] OK", its entry list matches
  `tests/golden/package/expected.windows.txt`, `reuse` linted it clean, and it replayed every
  committed scenario that `cargo xtask package` replays with the packaged `gamectl` against its
  golden chain, the seven under `scenarios/s3/` included, all "ok" (24 in all; the three nightly arms
  are not among them).
- The first zip, of `5e62ba7` (`D:/build/demo3/pharmakos-s3-5e62ba74-windows-x86_64.zip`, SHA-256
  `7cf6e5cc2e4f5cfd0c28f0f55db3a70fc4456dbb8c97b94097479a5db6fe5b89`, the same size and entries),
  is kept beside it and is not the demo's.
- Linux: the `pharmakos-linux` artefact of CI's `package (ubuntu-24.04)` job in `main`'s push run for
  `a8aa864a`, run 46036162824, green (not downloaded for this sheet).

The zip holds `Pharmakos.exe`, `gamectl.exe`, the client library, `library/` (five templates),
`rules/` (revision 8, `rules_hash` `641055eb0d38815a`) and `README.txt`, whose first lines are the
note on unsigned builds ("THIS BUILD IS UNSIGNED", then "More info", then "Run anyway"). It holds no
scenario files.

**Unzipping and folders.** Extract the whole zip into a folder of its own. The match cache is under
`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`, the lobby's `last_match.txt` and the log under
`%APPDATA%\Godot\app_userdata\Pharmakos\`.

**Pressing Ready.** "Ready — end the Lull" ends a Lull at once: Easy is always ready. **Ready with
nothing submitted files the safe playbook for you** ("You said Ready with nothing sealed, so the safe
playbook was filed for you."), so in every Lull press "Submit" first and wait for "Submitted. This is
now your sealed order." The editor carries last round's text into the next Lull; it still has to be
submitted again.

**The playbook, round by round.** The demo builds it in the editor from the guided pick. Here is the
text it reaches in round 4, so a Load can stand in for any step that misbehaves:

```jsonc
// The S3 demo's playbook, seat 0 on Probation's seed 20, as round 4 seals it.
// demo: the last stand waits for the flee rule until it is dragged above it.
{
  "schema_version": {"major": 1, "minor": 0},
  "meta": {"title": "S3 demo", "author_kind": "HUMAN"},
  "kind": "PLAYBOOK",
  "declarative": {
    "route": [
      {"label": "beyond",
        "interface": {"beacon": {"beacon_id": "b_00"}, "rows": [
          {"add_build_target": {"target": {"blueprint_id": "autocannon",
            "anchor": {"voxel": {"x": 218, "y": 28, "z": 29}}}}}]},
        "timeout_ms": 60000, "on_fail": {"action": "SKIP"}},
      {"label": "stone",
        "place_beacon": {"at": {"toward": {"feature": {"feature_id": "vent_232_96"}}}},
        "timeout_ms": 120000, "on_fail": {"action": "SKIP"}},
      {"label": "place_power", "place_beacon": {"at": {"covering": {"vent": {"rank": "NEAREST", "coverage": "UNCOVERED"}}}, "tags": ["power"],
        "initial": {"mandate": {"build": {"targets": [
          {"blueprint_id": "generator", "anchor": {"on": {"covered": {}}}, "rotation_quarter_turns": 0, "order": 1}
        ], "repair_threshold_pct": 60, "terraform": "NONE"}, "roe": "RETURN_FIRE"}}},
       "timeout_ms": 60000, "on_fail": {"action": "SKIP"}},
      {"label": "hold", "hold": {"ms": 30000}}
    ],
    "handlers": [
      {"id": "last_stand", "when": {"all": {"items": [{"cmdr_hp_pct": {"cmp": "LE", "pct": 30}}, {"cmdr_took_damage_within": {"ms": 3000}}, {"beacon_has_structure": {"beacon": {"beacon_id": "b_00"}, "blueprint_id": "generator"}}]}},
       "body": [{"label": "last_stand_home", "move": {"to": {"beacon_anchor": {"beacon_id": "b_00"}}}}, {"label": "last_stand_wait", "hold": {"ms": 15000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 1},
      {"id": "flee", "when": {"all": {"items": [{"cmdr_hp_pct": {"cmp": "LE", "pct": 40}}, {"cmdr_took_damage_within": {"ms": 3000}}]}},
       "body": [{"label": "flee_to_safety", "move": {"to": {"safest": {}}}}, {"label": "flee_wait", "hold": {"ms": 8000}}],
       "resume": "CONTINUE", "cooldown_ms": 30000, "max_fires": 3}
    ]
  },
  "on_death": {"on_respawn": "CONTINUE"},
  "fallback": {"shadow": {"beacon": {"safest": {}}}}
}
```

Each round's text, as the round probes sealed it:

| Round | The route | `gamectl verify --preset probation` (round 1's snapshot) or the probe's seal |
|---|---|---|
| 1 | Hold & Build's two steps, `place_power` and `hold`, its threshold set to 60, and the two rules | "qualifies yes (9 of 128 size units)"; sealed through `submit_plan` |
| 2 | adds `stone` before `place_power` | qualifies (10 of 128); sealed through `submit_plan` in round 2 |
| 3 | adds a `spire` visit first: `b_00`, `{"queue_structure": {"blueprint_id": "resonance_spire"}}` | E0507 in rounds 1 and 2 (no licence); in round 3's Lull no code refuses it |
| 4 | replaces `spire` by `beyond`, as above | E0416 while the core's sphere is 24; in round 4's Lull, once the Spire stands, no code refuses it (the whole-text probe's round-4 seal named only E0513, from its `spire` row) |

- About eighty minutes: Ready ends each Lull at once; the Pushes run 3, 5, 8 and 8 minutes at 1×,
  which "4x" and "Skip" shorten.

## The lobby

1. **Launch the game with four rounds.** From a terminal in the unzipped folder run
   `Pharmakos.exe -- --rounds 4`.
   - See: the lobby offers "New match: Probation" and a **Playtest mode** check button, off. Tick
     Playtest mode (its line says what it keeps, "in memory only"), then press "New match:
     Probation".
   - The match: two seats, you at seat 0 and Easy at seat 1, on Probation's seed `0x0000000000000014`
     (20; `MATCH_SEED` in `godot/scripts/host_link.gd`, held to `PROBATION_SEED` in
     `crates/sim/tests/probation_seed.rs` by `crates/client-gdext/tests/godot_project.rs`), the rules'
     3 / 5 / 8-minute ladder with round 4 taking its last entry, and untimed Lulls: "Round 1 - LULL"
     with no countdown. `--rounds` is a test-only launch option the lobby does not show (`ROUNDS_ARG`;
     the default is `ROUND_LIMIT` 3, the fun gate's match).
   - Proof: `crates/gateway/tests/probation.rs` `a_probation_lull_is_untimed_by_rule_and_a_custom_lull_never_is`
     and `the_lobbys_line_is_one_this_gateway_reads`, run for this sheet, "ok";
     `crates/client-gdext/tests/godot_project.rs` (60 tests, "ok"); `godot/scripts/watch_check.gd`
     `_rounds_cases` in CI's `client extension` jobs. Four rounds and the check button on screen: by
     hand only.

## Round 1's Lull: the guided pick and the rule list

2. **The guided pick (section 1.1 step 1).** The wizard opens by itself. Under the templates, "Hold &
   Build" is marked as the place to start, with the gateway's reason: "Start here: this places a
   beacon by the nearest heat vent you do not cover and builds a Generator on it, which raises your
   power, and power is what every later structure draws." The other four are still listed. Take it
   and press "Use this playbook".
   - See: the size meter under the editor reads "9 of 128 size units" once the rules below are in
     (`gamectl verify --preset probation` on round 1's text; Hold & Build alone reads less).
   - Proof: `crates/gateway/tests/probation.rs` `the_first_lull_marks_the_advisors_start_with_its_reason`
     and `the_guided_template_list_answers_its_golden` (`tests/golden/gateway/list_templates_guided/`),
     run for this sheet, "ok"; Easy names Hold & Build whenever the library lists it
     (`crates/operator/src/easy.rs`, `START_HERE_WHY`). The meter on screen: by hand only.
3. **The rule list (step 2).** Each rule is a sentence with chips.
   - Click the repair threshold's chip (50) and type 60, then press "Set this value": the gateway's
     verdict comes back on the row.
   - Add a rule below the flee rule, `last_stand`: the commander's hit points at most 30 % and damage
     taken within 3 s, body "go to the core, hold 15 s", one fire. See W0702 and its connector to the
     flee rule: "`last_stand` fires only while `flee` cannot: `flee` comes first, and its condition is
     true whenever this one's is, so this rule gets a turn only during `flee`'s cooldown or after its
     last fire." Drag it above the flee rule: the connector goes away.
   - Add a condition to `last_stand` through family, predicate and parameters: own beacon, has a
     structure, Generator, the beacon `b_00`. It stays true of no tick in this match (the core holds
     no Generator), so the rule never fires.
   - Open the entry list: *queue a Resonance Spire* is greyed with its lock and the gateway's reason,
     "needs the Resonance Spire licence, which the Ledger awards at a settlement where your
     Generators run on two vents".
   - The beacon covering the start vent is Hold & Build's own first step (`place_power`, "the
     nearest vent you do not cover", with a Generator on the vent it covers), so no step is added
     for it.
   - Type a comment line above the playbook, as in the text above.
   - Proof: `tests/golden/verifier/w0702_a_rule_an_earlier_rule_always_wins/` and
     `crates/verifier/tests/verifier.rs`
     `a_rule_is_warned_unreachable_only_when_its_condition_implies_an_earlier_one`; W0702 on this text
     with the rule below flee, and none above it, by `gamectl verify --preset probation` for this
     sheet; the lock's words, `crates/plan-core/src/strings.rs` `VOCABULARY_NEEDS_SPIRE_LICENCE`. The
     chips, the drag and the pickers on screen: by hand only (`fixp` #211's typing fix is on this
     build).
4. **Save and Load (step 3).** Press "Save as...", save it as `s3-demo.jsonc`, close it, and "Load..."
   it back: the comment is still there. Then load a copy with one more route step,
   `{"label": "flag", "set_flag": {"name": "ready"}}`: Load refuses it with E0003 at
   `/declarative/route/<N>/set_flag`, "`set_flag` is reserved: the schema holds it back for a later
   stage or version, and this build does not read it." Load the good file again.
   - Proof: `gamectl verify` on that copy, for this sheet: E0003 at its pointer;
     `tests/golden/verifier/e0003_reserved_construct/`; the comment round trip,
     `tests/golden/plan-core/`. On screen: by hand only.
5. **Submit, then Ready.** "Submitted. This is now your sealed order.", then "Ready — end the Lull".

## Round 1's Push: the start vent

6. **The beacon covering the start vent (step 4).** The commander walks out and places it, and its
   build drone raises the Generator on the vent.
   - Headless, round 1: `beacon_placed` for seat 0 at tick 721 and its Generator's
     `structure_completed` at 1742 (round probe 2); `ore_delivered` for seat 0 at 1489 and 3023.
   - **The brownout and its memo are not in this Push**: no probe saw `beacon_browned_out` for
     either seat. Shown staged ("Beside the demo").
   - Proof: `scenarios/s3/toward-stepping-stone` (`beacon_placed` by tick 721, `structure_completed`
     by 1682), replayed by the package, "ok".

## Round 2: the stone and the licence

7. **Lull (step 5).** Add a step before `place_power`: Place beacon toward the second vent,
   `vent_232_96` (`"toward": {"feature": {"feature_id": "vent_232_96"}}`), the stepping stone.
   `place_power` then covers the second vent from the stone, with its Generator. Submit ("qualifies",
   10 of 128), then Ready.
8. **Push.** The commander places the stone and then the beacon on the second vent; its Generator
   goes up.
   - At round 2's settlement your Generators run on two vents. The recap shows "Licence awarded:
     Resonance Spire.", and the first-licence memo: "Notice of licence. You ran Generators on two
     vents at settlement. You may now queue a Resonance Spire at any of your beacons."
   - Headless, round 2: `beacon_placed` for seat 0 at 4591 and 5316, `structure_completed` at 6133,
     and `licence_awarded` for seat 0 at 9600, the settlement (round probe 2); in every probe the
     licence came at round 2's settlement.
   - Proof: `scenarios/s3/resonance-spire` and `toward-stepping-stone` (`licence_awarded` for seat 0
     by tick 9600), replayed by the package, "ok"; `crates/gateway/tests/probation.rs`
     `a_licence_earned_at_a_settlement_reaches_the_recap_the_feed_the_briefing_and_the_vocabulary`,
     run for this sheet, "ok"; `crates/gateway/src/strings.rs` `memo_first_licence`.

**The money, measured on the final rules.** Rules revision 8 (`tune3` #250) moved two values,
`units.scout.vision_radius_voxels` 24 → 28 and the new `structures.resonance_spire_build_ms` 60000;
no price moved. So section 1.1's sum stands: three beacons ($180), two Generators ($160) and a build
drone at each ($40), $380, against the $200 start, one settlement and the starting drone's two $64
loads. Measured headless on `641055eb0d38815a`: the loads land at ticks 1489 and 3023 in round 1 (and
the third at 3799, in round 2), as `resonance-spire`'s committed event log has them, and every probe
placed all three and was awarded the licence at round 2's settlement. The probes see no treasury
figure, so the margin itself ("$43 to $58 is left") is the plan's arithmetic, not a measurement.

## Round 3: the Spire

9. **Lull (step 6).** The lock is gone. To the route's start add a visit to the core, `b_00`, with a
   `queue_structure` row for the Resonance Spire. Add a second at the same beacon: the verifier
   refuses it, E0513, "`b_00` already has a Resonance Spire queued earlier in this list; a beacon
   holds one `resonance_spire`, because two interfere with each other." Remove it. Submit, then Ready.
   - Proof: `tests/golden/verifier/e0513_a_second_spire_at_one_beacon/` and
     `e0507_a_spire_without_its_licence/`.
10. **Push (step 7).** The commander walks to the core and stays for the 5 s queue
    (`interface_times.queue_structure_ms`); the Spire's $120 leaves the treasury at once and its 3 kW
    draw starts. The core's fabricator raises it over 60 s (`structures.resonance_spire_build_ms`); on
    completion the core's sphere ring is drawn larger, 24 voxels to 36 (decision 6).
    - Headless, round 3: `structure_queued` for seat 0 at 9731 and `structure_completed` at 10929
      (round probe 3); in the whole-text probe 9816 and 11013, as in `resonance-spire`.
    - **Easy does not raid.** No probe saw `attack_launched`, `unit_destroyed` or `commander_died` for
      either seat in four rounds; Easy's scouts first found your beacons in round 5 of the eight-round
      probe. Shown staged ("Beside the demo").
    - Proof: `scenarios/s3/resonance-spire` (the chain pins the Spire queued at 9816 and finished at
      11013); `crates/sim/tests/spire.rs`.

## Round 4 and the end

11. **Lull (step 8).** The frozen snapshot holds the larger sphere. Replace the `spire` visit with the
    `beyond` visit: a Build target, an autocannon at (218, 28, 29), 30 voxels east of the core. The
    verifier accepts it: E0416 is gone. Submit, then Ready.
    - Proof: the whole-text probe's round-4 seal named no E0416; `resonance-spire`'s round-by-round
      codes (`crates/gamectl/tests/scenarios.rs` `the_s3_scenarios_pin_their_beats`).
12. **Push.** A drone builds the autocannon at once. The match ends at round 4's limit.
    - Headless, round 4: `structure_queued` for seat 0 at 19201 and `structure_completed` at 19842,
      `final_audit` and `match_ended` at 28800 (the whole-text probe).
13. **The survey and the bundle.** The playtest survey asks its three questions: "Play another
    round?", "Did you understand why your playbook did what it did?" and "What did you most want to
    change during a Push?". Answer them, press "Export playtest bundle" and pick a file: "Playtest
    bundle written to <path> (<N> bytes). Read it, then send it by hand."
    - The main session opens the zip beside the game: `manifest.json`, `survey.json`, `metrics.json`
      and `diagnostics.log`, and no playbook, draft, notebook or cache file.
    - Proof: `crates/client-gdext/tests/playtest_bundle.rs` `the_bundle_holds_exactly_its_four_entries`
      and `no_marker_reaches_the_bundle` (22 tests, "ok"), and `crates/client-gdext/tests/no_network.rs`
      (9 tests, "ok"), run for this sheet. The files' contents: by hand only.

## Beside the demo

Run these from a checkout at `a8aa864a`, with your own target directory. Each was run for this sheet on
the owner's i7-9800X (not as a timed run); the outputs quoted are from those runs, first on `5e62ba7`
and, for the probes and the scenarios, again on `a8aa864a`'s zip.

### Step 4's brownout, staged

**This beat is staged, and not played.** A Generator has no draw row, a placed beacon is net zero, and
the Quartermaster approves nothing that draws past the headroom (`crates/sim/src/economy.rs`
`SingleTreasury::approve`), so a seat's draw cannot outrun its supply by buying: a brownout needs its
supply to fall. On this beat's money no route was found. No probe browned out (rounds 1 to 8). One
try at a later route, a LOW-priority Survey beacon with seven scouts on the nearest seam in round 4
and both Generator beacons recycled after it, recycled both (ticks 24416 and 25601) but could not
place the Survey beacon first, so nothing was shed. The known route is S1's:

```sh
gamectl scenario run scenarios/s1/brownout-by-recycle.scenario.jsonc
```

which the package replayed, "ok" (the Survey beacon on LOW priority, the covering beacon recycled at
tick 4911, the shed at 4912, on the golden seed). The memo is the gateway's, in a Probation match only:

```sh
cargo test --release -p pharmakos-gateway --test probation -- a_brownout_issues_the_shortfall_memo_and_a_recycle_is_no_dark_stake
```

printed "ok" for this sheet. Its words: "Notice of shortfall. Your draw exceeded your supply and the
grid shed load in the order you set. Build a Generator on a vent, or recycle what you cannot feed."
Changing the beat or decision 4's trigger is the owner's ruling (question 5 below).

### Easy's raid and the first death, staged

**This beat is staged, and not played.** Easy's raid is not shown live. On seed 20 with this sheet's
playbook, the eight-round probe saw Easy's first `enemy_beacon_found` at tick 28991 (round 5), no
`attack_launched` for Easy in eight rounds, and one unit of Easy's destroyed (tick 47871, round 6,
credited to seat 0). The nightly Turtle on seed `0x0102030405060708` (the Turtle at seat 0, Easy at
seat 1; `scenarios/nightly/turtle.scenario.jsonc` with that seed and six rounds, not committed) shows
the raid:

| Tick | Round | Event | Seat |
|---|---|---|---|
| 28021 | 4 | `enemy_beacon_found` | 1 |
| 28646 | 4 | `enemy_beacon_found` | 1 |
| 29432 | 5 | `attack_launched` | 1 |
| 33032 | 5 | `attack_ended` | 1 |
| 38401 | 6 | `attack_launched` | 1 |
| 38718 | 6 | `attack_ended` | 1 |
| 38914, 47051, 47312 | 6 | `unit_destroyed` (each `kill_credited` to seat 0) | 1 |

`gamectl scenario run scenarios/nightly/turtle.scenario.jsonc --operator easy --seeds
scenarios/nightly/seeds.txt --format json` on the zip's `gamectl` gives Easy's launches on three of
the 20 seeds, rounds 5 and 6 on `0x0102030405060708` and `0x9e3779b97f4a7c15` and round 6 on
`0xa708a824f612c926`: no Easy launch before round 5. No commander died in that match. The first
death's memo is shown by `crates/gateway/tests/probation.rs`
`a_memo_fires_once_survives_a_resume_and_never_in_a_custom_match`, run for this sheet, "ok"; its words:
"Notice of absence. Your Keybearer was lost and will return at your core after the usual delay, or at
your nearest beacon if the core is gone. Nothing you sealed changes in the meantime." A commander
hunted to death is `scenarios/s2/hunt-the-commander` (`commander_died` for seat 1).

### The Spire's destruction, staged (section 1.1 step 9)

**This beat is staged, and not played**, as section 1.1 says: no scripted match is sure to destroy the
Spire. `lic`'s scenario stages it on Probation's seed: seat 0 earns the licence as in
`resonance-spire` and queues the Spire at its stepping stone, `b_02`, an unarmed beacon well forward;
seat 1 seals an Attack beacon aimed past it.

```sh
gamectl scenario run scenarios/s3/spire-lost.scenario.jsonc
```

printed "spire-lost: 5 assertions, all held." on the zip's `gamectl` (`rules_hash 641055eb0d38815a`,
its chain included). Its committed event log, `tests/golden/scenarios/spire-lost/expected.events.txt`
(held by `crates/gamectl/tests/scenarios.rs`), read past its step, visit and ore lines:

| Tick | Event | Seat |
|---|---|---|
| 726, 1687 | `beacon_placed`, `structure_completed` (the start vent's Generator) | 0 |
| 4626, 5291, 6036 | `beacon_placed` (the stone, `b_02`), `beacon_placed`, `structure_completed` | 0 |
| 9600 | `licence_awarded` | 0 |
| 10511, 11709 | `structure_queued`, `structure_completed` (the Spire, at the stone) | 0 |
| 16000 | `attack_launched` | 1 |
| 22324 | `kill_credited` (seat 1), `structure_destroyed` (the Spire), `wreck_left` | 1, 0, — |
| 22501 | `kill_credited` (seat 1), `beacon_destroyed` (the stone) | 1, 0 |
| 22571, 23171 | `commander_died`, `commander_respawned` | 0 |
| 22597 | `attack_ended` | 1 |

On tick 22324 the stone's sphere is its base radius again: the sim derives the radius from the
structure table, and `crates/sim/tests/spire.rs`
`the_sphere_returns_to_its_base_on_the_tick_the_spire_is_destroyed` holds the rule.

### The certification on the final rules

**ITEM-225-PENDING.** The certifying runs on the demo's tree, `a8aa864a`, were retaken by the main
session after #255 (the three message strings above), and their record is decisions-log item 225; its
figures replace the table below when the main session sends them, and none is written here before
then. Until then the table is item 223's, the runs of 2026-11-27 on `main` at `7a604e22` (#253's
merge), on the owner's i7-9800X under Windows and WSL2 Linux, three repetitions a system, the median of
the three p99s, the slower system deciding, copied as item 223 states it. **It is not the record of
runs on `a8aa864a`.**

| Gate | Bar | Windows | WSL2 | Deciding |
| --- | --- | --- | --- | --- |
| G2's repath p99 | ≤ 5 ms | 0.919 (retake) / 0.960 / 0.907 ms | 0.878 / 0.875 / 0.885 ms | **0.919 ms**, Windows |
| P3's one-leg estimate p99 | ≤ 1 ms | 0.523 (retake) / 0.499 / 0.494 ms | 0.484 / 0.473 / 0.470 ms | **0.499 ms**, Windows |
| P3's accuracy (real commanders, three seeds) | p90 ≤ 15 %, never optimistic | from the serve min 0, p90 0, max 0 per mille; from the send min −48, p50 −2 per mille | identical | **0 per mille from the serve, never optimistic** |
| P1 QUICK p99 at 128, worst shape (`route_guards_24_node`), canonical bytes | ≤ 5 ms | 3.364 / 3.349 / 3.315 ms | 3.101 / 3.158 / 3.094 ms | **3.349 ms**, Windows (33 % under) |
| P1 FULL p99 at 128, worst shape (`handlers_24_node`) | ≤ 50 ms | 7.523 / 7.618 / 7.531 ms | 7.330 / 7.320 / 7.350 ms | **7.531 ms**, Windows |
| The editor's end-to-end wait (`verify_plan`, the worse of QUICK and FULL, worst shape `handlers_24_node`; item 162 (1)) | ≤ 50 ms | 8.142 / 8.527 / 8.093 ms | 8.056 / 8.086 / 7.893 ms | **8.142 ms**, Windows |
| P1 QUICK, the 129 committed cases pooled | ≤ 5 ms | 0.325 / 0.322 / 0.324 ms | 0.270 / 0.262 / 0.269 ms | 0.324 ms, Windows |
| G3′-real at S2's load, tick p99 / mean (published, unstamped; certifies nothing) | 25 / 12.5 ms | 13.172 (retake) / 13.049 / 13.102 ms; mean 4.821 / 4.793 / 4.804 ms | 13.257 / 13.218 / 13.219 ms; mean 4.941 / 4.937 / 4.943 ms | 13.219 / 4.941 ms, WSL2 (published) |
| G3′ with raids, `budget_128` | 25 / 12.5 ms | 11.742 / 11.778 / 11.792 ms; mean 4.675 / 4.674 / 4.680 ms | 12.109 / 12.050 / 12.123 ms; mean 4.820 / 4.816 / 4.817 ms | **12.109 / 4.817 ms**, WSL2 |
| G3′ with raids, `full_eval` at 128 | 25 / 12.5 ms | 11.902 / 11.908 / 11.921 ms; mean 4.718 / 4.705 / 4.710 ms | 12.092 / 12.161 / 12.110 ms; mean 4.839 / 4.834 / 4.840 ms | **12.110 / 4.839 ms**, WSL2 |

The rules hashes as printed: the committed table `641055eb0d38815a`, the G3′ bench table
`b947507e651166ba`, the layout `46c4018ed990aa15`, the seed `0x00000000ca5caded` (item 223 (2)). The
size budget stayed at 128 through `tune3`. **ITEM-225-PENDING:** the re-run on the final rules that
the plan gave this lane (sections 5.1 item 5 and 5.2 item 6, item 162 (5)) is item 225's, on
`a8aa864a`; this lane took no timed run. **P1 QUICK's margin** on item 223's runs: 3.349 ms, 33 % under
its 5 ms bar, the closest any gate came (item 223 (3)); item 225's figure replaces it. Not certified by
these runs: P6 (section 5.3, the record above), and P1's byte identity and fuzz run, below.

### Section 5.2's items 2 and 3, on the final tree

- **Byte-identical across systems:** `main`'s push run 46036162824 on `a8aa864a`, every job green but
  the skipped DCO walk (a push run has no pull request's commits to walk); its `cross-OS determinism
  guard` reported "167 kind(s) byte-identical on every leg that has them", among them 130 verifier
  files on all three legs (the catalogue and 129 reports,
  `golden/verifier/budget_128/actual.report.json` included).
- **10 000 fuzzed playbooks, no panic:** the nightly run 45997206064, the latest (scheduled, on
  `7a604e22`; #255 changed no verifier source, and no nightly has run on `a8aa864a` yet), ran `cargo xtask checked-test --package pharmakos-verifier --test fuzz --ignored
  --nocapture`: `ten_thousand_playbooks_come_back_as_reports ... ok` and
  `two_hundred_forty_thousand_two_node_edits_that_qualify_compile ... ok`, "2 passed".

### The nightly report

The same nightly run, 45997206064, on `7a604e22`, every job green: the three arms, the verifier fuzz,
G4's chains on Windows, Ubuntu and macOS and their cross-OS compare. Its "nightly scenarios (not yet
enabled)" job was skipped, so the arms run on the schedule (`NIGHTLY_SCENARIOS_ENABLED` is set). Each
arm's line reads "20 seed(s) played, the arm the sole winner of <N>":

| Arm | Sole wins vs Easy | Band (an alarm above 65 %) | Acknowledged |
|---|---|---|---|
| Rusher | 16 of 20 (80 %) | **over**: the job warned "rusher won 80% of seeds, above the 65% alarm band — open a tuning task" | open, under the owner's answers 22 and 26 (items 213 and 217) |
| Hunter | 9 of 20 (45 %) | within | — |
| Turtle | 0 of 20 | within | — |

Easy launches a raid in 3 of the 60 matches, all against the Turtle (item 219 (8)); the run above
found the same three. These are shown as measured, not tuned and not explained away (answer 26).

## Deviations from section 1.1

1. **Step 4's brownout is staged**, not played: no route fits the beat's money (above).
2. **Easy does not raid in this match** (step 7), and the commander does not die, so the first death's
   memo is not shown live; both are staged.
3. **The beacon covering the start vent is Hold & Build's own step** (step 2's last action and step 4),
   not a step the owner adds: the guided pick places it.
4. **The lock's words** are the product's: "needs the Resonance Spire licence, which the Ledger awards
   at a settlement where your Generators run on two vents", not the plan's paraphrase.
5. **The condition of step 2** goes on the new rule, `last_stand`, where it changes nothing in play.
6. **Round 4's Build target** replaces round 3's Spire row, since a Spire standing at the core makes
   the row E0513.
7. **The Spire's destruction is staged** (step 9), as section 1.1 says.

## Told to the owner, not asked

Each is reported as the decisions log states it, with its item. Each is the main session's call, which
the owner can veto, unless it quotes the owner.

1. **Easy's changed seals** (item 169 (11) (a), (12) (a)): "where it sealed the safe playbook, it
   seals Hold & Build on those seats, and its hold went from 9,900 to 16,100 ms in `against-easy`."
   From `opr3` #205 (item 180): Easy queues the Spire, steps with `toward` and places a Survey post.
2. **The new refusals at load** (item 169 (9), (12) (b)): a rules table with a diagonal step cheaper
   than a cardinal or dearer than two, a dearest step no path's `i32` can hold, a repath cap of 0, or a
   row past the sim's `i32` of milliseconds (`RulesError::RowPastTheSim`), "which only a table the
   committed rules do not match meets": the zip's rules do not meet them.
3. **The allowance and the repath cap** (item 169 (12) (c)): the allowance per re-ask is 20 ticks
   (1 s) at the committed cap of 16 (20, 39, 77, 154 and 308 at caps 16, 8, 4, 2 and 1 at two seats);
   "never later than promised" is counted from when the route is served (item 169 (3) (a)).
4. **`estimate_route` refuses with `PHASE_CLOSED` outside a Lull** ("Refuse in a Push", item 169 (3)
   (b)), and **`get_map_summary` omits `travel_ms` and `reachable` outside a Lull** ("Omit them in a
   Push", item 169 (3) (c); item 170). The editor asks only in a Lull, so nothing in this demo changes.
5. **Probation's seed stays 20** though seed 13 also passes ("Keep 20", item 168 (4); item 169 (7)).
6. **`charge_lost`** is announced for every loss of a carried charge and not refunded ("Keep", item
   168 (6)); a lead perched on an enemy wall with no way off ends `Sealed` (item 168 (7)).
7. **The patch undo's form** is the gateway's own, not an RFC 6902 patch ("Keep as built", items 167
   (4) and 168 (3)), and the forward patch refuses an unknown member.
8. **Spec section 6's Survey sentence** binds the refresh of sightings already held, not a first
   discovery ("Refresh only (Recommended)", 2026-11-22, item 173 (1)); the scout count stays 2 until
   S5.
9. **Items 166 (1) to (6)** (the repair at the end of the Voxels phase, one door for every snapshot,
   the breach search on the walkers' moves, `charge_lost`): none changes what this demo shows, since
   no breach and no charge happens in it.

## Owner rulings at the demo

One batch (decisions-log item 168 (12), hold and batch). Each comes with the lane's recommendation and
its class. None is answered here.

1. **The rows `tune3` kept (decision 19's agenda).** Rules revision 8 moved scout vision 24 → 28
   (answer 21) and added `structures.resonance_spire_build_ms` 60000 (answer 19, marked for this
   review); every other row is kept at its value "with no balance re-run under item 213's answer 22"
   (135 marker lines, item 219 (12)), and so are the eight delegated S3 rows (`match.decision_tick_ms`
   250, `structures.resonance_spire`, `verifier.handler_cooldown_min_ms` 5000, `max_fires_max` 8,
   `notebook_max_chars` 4000, `verifier.reach_memory_ms` 180000, `verifier.size_budget_units` 128 and
   `locomotion.climb_surcharge` 4). No row has a balance report line: `tune3` made no re-run (item 219
   (10)). *Recommendation:* keep revision 8 for S3, and make the balance re-read the first tuning task
   once the pause lifts, as answer 26 says. *Class:* operator: priority.
2. **The Spire's numbers and the charge's damage** ($120, 500 HP, 3 kW, the sphere 24 to 36, a 60 s
   build; `combat.charge_damage_hp` 60), not read against a duel report (item 219 (10)).
   *Recommendation:* keep for S3. *Class:* operator: intent.
3. **The nightly bands and Easy's raids, as measured** (answer 26): the Rusher 16 of 20, over its
   band; Easy raiding in 3 of 60 matches, all against the Turtle, none before round 5.
   *Recommendation:* acknowledge the Rusher's band as open for S3's close (AGENTS.md section 10 item
   5), with both first after S3 as answer 26 says. *Class:* operator: risk.
4. **R11 and Easy's Survey post.** No raid reached a newcomer-shaped seat in a Probation-length match:
   on seed 20 Easy first sights the owner's beacons in round 5, and in the nightly its earliest launch
   is round 5. *Recommendation:* keep the Survey post (no revert by cut 3's unit). *Class:* operator:
   intent.
5. **Step 4's brownout.** No route fits the beat. *Recommendation:* keep the beat staged for S3 and
   leave decision 4's trigger as it is. *Class:* operator: intent.
6. **The money on seed 20.** Every probe earned the licence at round 2's settlement on the final
   rules. *Recommendation:* no cure; seed 20 stands. *Class:* operator: intent.
7. **The memo copy as first written** (register S3-33, "owner, at S3's demo"). *Recommendation:* keep
   it for S3, polished at S6 (spec section 19). *Class:* operator: intent.
8. **The other markers that read "owner, at S3's demo"**: S3-27 (one voxel as a structure's look),
   S3-28 (Survey's re-pricing cadence, cell and refresh), S2-19 (`TOP_UP_TICKS`), S1-52
   (`CORE_DIG_MAX_DEPTH` 4), `BREACH_COLUMN_COST`, the empty Attack and Defend arms' readings, the
   commander's speed, the repath cap of 16, the broadphase cell, the audit's who, the three world
   totals, the balance check's seeds and flags, and the repair threshold of 50.
   *Recommendation:* keep each at its value and re-date it to the stage the re-strategy interview
   plans. *Class:* operator: priority.
9. **`BENCH_SEED` (S1-49, "at S3's exit") and `BREACH_EXPANSIONS` (S3-32)**, with item 223 (7)'s
   figures: E12's rule within on all six readings (101.4, 97.0, 107.3, 98.3, 99.8, 100.1 %); the
   breach probe's searches spent the bound (16,385 states) in minutes 1 and 2, the slowest 9.807 ms,
   and G3′ with raids certified at 12.109 ms. *Recommendation:* keep `BENCH_SEED` and keep 16,384.
   *Class:* operator: intent.
10. **R13, the systems' gap**, open for the owner (item 223 (3), (6)): every gap under the 10 to 20 %
    R13 names, the largest P1 QUICK's 8 %, the 2026-11-26 tail a diagnosis and not a proof; the
    lesson, "restart Windows first". *Recommendation:* keep R13 open until a later certifying night
    reads clean after a restart, and fix the during-run poll first (item 223 (4)). *Class:* operator:
    risk.
11. **P6** (the line at the top). *Recommendation:* if the record shows no miss, close S3 on it,
    stated as AI-driven and not a person's time. *Class:* operator: policy.
12. **Decision 16, `authoring budgets (ubuntu-24.04)` as a required check.** It has passed on six
    push runs of `main` in a row since `cert3` merged, seven with `a8aa864a` (`9893031e`, `23a76625`,
    `4ce711cc`, `95cd0301`, `7a604e22`, `5e62ba7`, `a8aa864a`). *Recommendation:* make it required. *Class:* operator: policy.
13. **Decision 14.** The view's delta mode (S2-12) and the recompute key (S1-34) are measured by
    `cert3`'s watch and their markers already read S7. *Recommendation:* confirm S7. *Class:* operator:
    priority.
14. **Decision 10.** *Recommendation:* cut `v0.1.0-rc.1` from the commit this review closes S3 on.
    *Class:* operator: policy.
15. **Whether the merge delegation runs on into S4** (item 152 (2), decision 19; CLAUDE.md: it returns
    to the owner at this review). *Recommendation:* renew it on the same terms for the stage the
    re-strategy interview plans. *Class:* operator: policy.
16. **Item 162 (8)'s parked items** (the radar lab, macOS in previews, whether the owner's global
    release rules apply here, where the briefs folder is backed up, whether item 42's registrations
    were made). *Recommendation:* take them at the re-strategy interview. *Class:* operator: priority.

**Held for the re-strategy interview, not in this batch:** the order of `deps3` and the scouts'
routing after S3 (answers 24 and 30, items 217 (3) and 220 (5)); the decision tick's kinds'
disagreement (items 217 (6) and 223 (7)); the veto list of item 217 (6) (the sighting table's room,
each seat's list and avoid-area rooms and the interpreter's two ceilings, item 219 (1) and (5), and the
reading of answer 20); the balance re-read of every S2, economy, power and map row, the duel report's
reading and the lone carrier (items 217 (6) and 219 (10)); and the re-strategy itself (item 173 (7)).

## The fun gate's readiness (section 1.4), checked

| Item | Check | Result |
|---|---|---|
| 1. The gate's match is one click | `godot/scripts/host_link.gd`: `PRESET` "probation", `MATCH_SEATS` 2, `HUMAN_SEAT` 0, `ROUND_LIMIT` 3, the rules' ladder (3 / 5 / 8 min), `MATCH_SEED` 20; the gateway's untimed Lulls (`probation.rs`' tests above) | met: "New match: Probation" |
| 2. A first-timer can file a playbook | the guided pick (step 2); Ready with nothing sealed files the safe playbook and says so (`the_filing_line_says_what_ended_the_lull`, run, "ok"); every refusal a sentence (the verifier's catalogue and the gateway's string table) | met |
| 3. The four metrics recorded and exported, nothing else, no network | `playtest_bundle.rs` (22 tests) and `no_network.rs` (9 tests), run, "ok"; `crates/gamectl/tests/confinement.rs` in CI | met |
| 4. The zip opens on a clean machine | `clean launch (windows-latest)` and `clean launch (ubuntu-24.04)` green in run 46036162824 on `a8aa864a` (and in 46033625376 on `5e62ba7`); the unsigned note is `README.txt`'s first lines | met |
| 5. No numeric threshold written anywhere | no threshold in `playtest.gd`, `playtest.rs`, the playtest strings, this sheet or the tester's sheet | **SPEC3-PENDING.** **Not met in the spec at `a8aa864a`**: spec section 16's gate row still reads "Go if a majority answer yes and Push attention is a stated majority of the segment"; section 4.4's amendment ("the owner's judgement, on the playtest bundle's four metrics") has not landed there. It is being landed by `spec3`'s pull request (`docs/s3-spec`, number to follow), which merges before this one |
| 6. The tester's sheet exists | `docs/demo/fun-gate-sheet.md`: how to start the zip, turn on Playtest mode, and send the bundle by hand; no tester's name or address, and no channel | met |

## AGENTS.md section 10, item by item

| Item | Evidence | Source |
|---|---|---|
| 1. `cargo xtask ci` green on the matrix, the systems agreeing | every job of `main`'s push run 46036162824 on `a8aa864a` green (the DCO walk skipped, as on every push run); the guard, 167 kinds byte-identical | GitHub Actions |
| 2. Hashes committed, every movement explained | plan section 4.2's ledger and its amendments; the last re-bless is `tune3`'s (item 219 (11): two scenario chains, the 129 report goldens and the rules hash's goldens, the determinism chain not moved); `cert3` added six PNG goldens (item 218 (12)); nothing moved since | decisions log |
| 3. Golden playbooks round-trip, verify and render identically | `tests/golden/plan-core/` and `tests/golden/verifier/`, green on the matrix and compared across systems | run 46036162824 |
| 4. Headless scenarios assert events and hashes | 24 committed scenarios (all but the three nightly arms) replayed by the packaged `gamectl` against their golden chains, all "ok", the seven under `scenarios/s3/` included, on `5e62ba7`'s zip and again on `a8aa864a`'s | this sheet's packages |
| 5. The nightly adversarial scenarios | run 45997206064: the Rusher 16 of 20 (open, acknowledged), the Hunter 9, the Turtle 0 | the nightly |
| 6. The depth task and gate | **ITEM-225-PENDING.** G2 + P3 and P1 certified by item 223 on `7a604e22`, and retaken on `a8aa864a` by the main session, recorded in item 225; P6 by the closing rule, AI-driven, on the owner's ruling (`docs/demo/p6c-record.md`) | items 223 and 225; the record |
| 7. Docs in the same PR | **SPEC3-PENDING.** **Not all landed at `a8aa864a`**: none of section 4.4's spec amendments (the spec is unchanged there since 2026-10-09), which `spec3`'s pull request (`docs/s3-spec`, number to follow) is landing before this one merges: section 3's Probation paragraph, section 8's licence, section 13's chips, vocabulary and meter, section 14's Survey post, section 16's fun-gate row, sections 3, 16 and 17's v0.1 and the S3 and S4 rows, section 19's memo copy. AGENTS.md section 3 rule 4's S3 client pieces and section 9 item 11's authoring alarm have landed | the spec; AGENTS.md |
| 8. The owner reviews the demo | this review | the owner |

## Found while writing this sheet

- **#255 merged after the sheet's first commit** (`5e62ba7`). In shipped code it changes the text of
  two internal errors in `crates/gateway/src/serve.rs` (the control pipe failing) and one rules-load
  refusal in `crates/sim/src/attack.rs`; the rest is tests. The main session ruled (engineering:
  decided) that item 223's condition holds by its letter, so the sheet was moved to `a8aa864a`, the zip
  rebuilt on it and the certifying runs retaken on it (item 225).
- **AGENTS.md section 7** says the playtest bundle contains "survey, metrics, recording and the
  diagnostics log"; the bundle holds `manifest.json`, `survey.json`, `metrics.json` and
  `diagnostics.log`, and no recording (`the_bundle_holds_exactly_its_four_entries`). AGENTS.md is a
  contract file, so this lane does not edit it.
- **The register's prose on the repair threshold of 50** names three JSONC files; the marker stands on
  13 lines in 12 files (`library/hold_and_build.jsonc`, `scenarios/skeleton/against-easy.playbook.jsonc`,
  `godot/fixtures/instantiate_suggested.json`, the gateway's `instantiate_suggested` golden, the
  operator's six `seed-*` goldens and its `hold_and_build` wizard golden, and two lines of
  `crates/gamectl/tests/fixtures/save-v3-round-two-lull.json`).
- **`crates/sim/src/targeting.rs`' module doc** still says "S3's fog work" for S3-20, which decision 2
  moved to S4; for the next `crates/sim` lane.
- **Recycling a beacon puts `beacon_destroyed` on the bus** (the round-4 brownout try: `beacon_recycled`
  and `beacon_destroyed` on the same tick); the gateway issues no Dark Stake memo for it, as
  `a_brownout_issues_the_shortfall_memo_and_a_recycle_is_no_dark_stake` holds.
- **`NIGHTLY_SCENARIOS_ENABLED`** is set: the scheduled nightly skips its "not yet enabled" job and runs
  the arms. S2's sheet said it was unset; item 152 (1) 12 recorded the main session setting it to
  `true` on the owner's word, on 2026-10-28.
- A headless probe sees no treasury and no writ: a scenario assertion on a value (register M-05) would
  let a sheet measure the money's margin and name the beacon Easy's scouts came from.
