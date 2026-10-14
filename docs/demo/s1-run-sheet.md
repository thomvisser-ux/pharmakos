<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Stage demo run sheet: S1, the economy

The owner's review of the stage demo ends S1 (AGENTS.md section 10 item 8). This sheet walks the
demo of `docs/design/s1-plan.md` section 1.1 as the zip plays it, one step at a time: what to do,
what to see, and which test or scenario also proves the step, or "by hand only" where none does.

It is written against `main` at `49398cf` (#102, item 137's docs pull request, on `tune` #101's
`952fc0a`). The words in quotation marks are the product's own: the client's from
`godot/scripts/strings.gd` and `godot/scripts/lobby.gd`, the gateway's from
`crates/gateway/src/strings.rs`, the verifier's from its diagnostic catalogue, as the client shows
them. A number in angle brackets, such as `<N>`, is one the match decides and this sheet cannot
know. Where the build differs from section 1.1, the step says so, and "Deviations from section
1.1" at the end lists each difference for the owner's ruling. Row IDs such as S1-37 are rows of the
PLACEHOLDER register, `docs/placeholders.md`.

Section 1.1 asks for more than one match shows by itself: a deficit, a recycle and a timeout all
need the player to make them. The sheet makes them on purpose, one per round: round 1 places and
builds with the wizard, round 2 loads a committed playbook that recycles the Generator's beacon
into a deficit, and round 3 lets the Lull run out so the safe playbook is filed and digs the core's
seam below its rim. Steps that only a test can reach say so.

## Before you start

**The zip.** No S1 tag was cut. `release.yml` takes a release's notes from `CHANGELOG.md`'s
section headed `0.1.0-dev+skeleton`, a heading written into the workflow
(`.github/workflows/release.yml`, `heading="0.1.0-dev+skeleton"`), so a tag cut now would carry the
skeleton's notes. That heading is a carry for the next `.github/**` pull request. The demo uses a
package of `main` at `49398cf` built locally:

- Windows: `D:/build/s1-demo/pharmakos-s1-49398cf-windows-x86_64.zip`, 117 026 458 bytes, SHA-256
  `17a4b0345c2ef74be4f22b3cf6dde20dcbb159ed723e933161baf24102436c63` (also in
  `D:/build/s1-demo/SHA256SUMS.txt`). `cargo xtask package` built it, which also replayed every
  committed scenario with the packaged `gamectl` against its golden chain.
- Linux: the `pharmakos-linux` artifact of CI's `package (ubuntu-24.04)` job in `main`'s push run
  for `49398cf` (run 38168063262, green; the artifact is kept until 2027-01-09).

The zip holds `Pharmakos.exe`, `gamectl.exe`, the client library, `library/` (the three templates)
and `rules/`. It holds no scenario files.

**Unzipping.** As the skeleton's sheet says: on Windows extract the whole zip into a folder of its
own and run `Pharmakos\Pharmakos.exe` (the build is unsigned: "More info", then "Run anyway" if
SmartScreen asks); on Linux run `chmod +x Pharmakos.x86_64 gamectl` if the archive tool dropped the
bits, then `./Pharmakos.x86_64`. There is no macOS zip.

**Where things are kept.** Unchanged from the skeleton: the private match cache under
`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\` (Linux `~/.local/share/pharmakos/matches/`), and
the lobby's `last_match.txt` and the game's log under `%APPDATA%\Godot\app_userdata\Pharmakos\`
(Linux `~/.local/share/godot/app_userdata/Pharmakos/`). The newest `local-*` folder is the match
being played.

**Have ready:**

- A checkout of `main` at `49398cf` (the owner's clone at `C:/Users/PC/pharmakos` is enough). Round
  2 loads a copy of `scenarios/s1/brownout-by-recycle.playbook.jsonc`, which the zip does not ship.
  Copy it now to a folder of playbooks as `round2-brownout.jsonc`, and a second copy as
  `round2-e0601.jsonc` in which `"scout_count": 7` reads `"scout_count": 40` and
  `"allow_dormant_beacons": true` reads `"allow_dormant_beacons": false`.
- A plain-text editor and Task Manager.
- About fifty minutes. The first Lull runs 10:00 and the later ones 5:00, but Ready ends a Lull at
  once; the Pushes run 3, 5 and 8 minutes at 1×, which "4x" and "Skip" shorten.

**What only a person sees.** CI's `clean launch` jobs run the zip's smoke check and
`gamectl seat doctor` headless. Rendering, the GPU (Vulkan, Forward+), a firewall prompt at New
match and a console window for `gamectl` are a person's to note, as at the skeleton's demo, where
none of the last two appeared.

## The lobby

1. **Launch the game.**
   - See: the chooser, "New match" and "Credits", and "Resume last match" if an earlier match was
     left unfinished (`lobby.gd` shows it only when a match is remembered), with "Start a new match,
     or go on with the last one." No speed buttons and no "Orders" panel: the watch strip and the editor stay hidden until
     the host announces a match (`fixc`'s F6 work).
   - Proof: `crates/client-gdext/tests/godot_project.rs`
     `the_chooser_hides_the_watch_strip_and_the_editor_until_a_match_starts`; the shipped smoke
     check in the `package` and `clean launch` jobs.

2. **Start a second `Pharmakos.exe` beside the first (F3).** Leave the first at its chooser and run
   the same exe again, then close the second.
   - See: the build has no single-instance guard and no "already running" message (see Deviations).
     The owner deferred the guard until F3 recurs (item 129 (2) (c)); `fixc` found the second
     instance rendering in every run. Note whether the second window renders, hangs or says
     anything. Do not press anything in it: the two instances share `user://`, including
     `last_match.txt`.
   - Proof: by hand only. `godot_project.rs` `the_project_pins_the_settings_item_56_rests_on` pins
     the flushed log (`run/flush_stdout_on_print`), so a hang leaves its lines in the log.

3. **Press "New match".**
   - See: "Starting the match host...", "Connected. Waiting for the view...", then the vista and the
     status line "Round 1 - LULL  10:00", the timer counting down. The speed label is shown only in
     a Push. The match is two seats on the golden seed `0x00000000ca5caded`, you at seat 0 and Easy
     at seat 1, three rounds on the rules' 3 / 5 / 8-minute ladder (`godot/scripts/host_link.gd`
     `MATCH_SEED`, `MATCH_SEATS`, `ROUND_LIMIT`, `LADDER`).
   - Under "Your economy": "$200   supply 10 kW   draw 4 kW   headroom 6 kW", with "Supply and draw
     leave out each beacon's own base draw, which its key-core supplies." Under "Your next
     settlement": "Next BMI, at the band you hold now: $<N>. A prediction: ..."
   - Do (F6, by hand): turn the mouse wheel over the map to zoom in and out, and note that each
     notch zooms. `fixc` left the camera's wheel zoom to the owner's check by hand (S1's plan, task
     `fixc`); no test covers it.
   - Proof: `godot_project.rs` `the_lobby_hosts_a_three_round_match`; `crates/client-gdext/src/rig.rs`
     `the_first_lull_is_timed_by_its_own_row_and_later_lulls_by_the_other` (10:00 from
     `match.first_lull_ms`, 5:00 from `match.lull_ms`); `godot/scripts/watch_check.gd` in the
     `client extension (<os>)` jobs.

## Round 1's Lull: the wizard, the chips, the checks, submit

4. **Open "Hold & Build".** Under "Start from a template" are "Expand & Mine", "Hold & Build" and
   "Safe playbook", the rewritten templates: each places by a description, never by a voxel.
   - See: "Template: Hold & Build" and two pages, "Which heat vent the Build beacon covers: the
     nearest one you do not cover yet, or one by name" and "How long to hold by the new beacon, in
     game milliseconds", each with "suggested by the built-in operator" and "Why: ...". Easy's
     suggestion for the vent is a **name**, `{"feature_id":"vent_324_16"}`, not the description.
   - Proof: `crates/gamectl/tests/wizard.rs`
     `the_wizards_live_suggestion_qualifies_and_is_goldened_for_every_template` and
     `tests/golden/operator/wizard/hold_and_build/expected.parameters.json`;
     `crates/client-gdext/tests/wizard.rs` `the_pages_are_the_fixtures_parameters_as_they_came`.

5. **Put the description back.** On the vent page type
   `{"vent":{"rank":"NEAREST","coverage":"UNCOVERED"}}` and press "Send this value". Section 1.1's
   step 1, its chip and round 2's sentence all assume the description; with Easy's name the step is
   "covering Heat vent (324, 16)", the chip says "named", and what round 2 reads is pinned by no
   test.
   - See: the page shows the typed value without the mark.
   - Proof: `wizard.rs` `an_edited_value_is_sent_exactly_as_typed_and_alone`.

6. **See W0701, then undo it.** On the hold page type `200000` and press "Send this value", then
   "Use this playbook".
   - See: "The template's playbook is in the editor. Undo takes it back." Under "Checks" a row
     "Warning", "There is not time for this whole route this round.", "W0701 at
     /declarative/route/1". Round 1's Push is 180 000 ms. Press "Undo", which takes the template's
     playbook back, open "Hold & Build" again, repeat step 5, send `30000` on the hold page and press
     "Use this playbook".
   - Proof: `tests/golden/verifier/w0701_a_route_longer_than_the_segment/expected.report.json`
     ("the route needs at least ... ms if every step runs, and the coming segment is 180000 ms.").
     The hold as the cause: by hand only.

7. **Read the playbook.** Under "What the playbook says", step 1 reads "place_power: place a beacon
   at a site covering the nearest heat vent you do not cover yet (nearest by travel, read when the
   step starts), tagged "power", on a Build mandate (14.5 s on site)." Section 1.1's words, "the
   nearest vent you can cover", are the Alt-click menu's (step 9) and round 2's (step 17).
   - Proof: `tests/golden/gateway/demo_hold_and_build_render/expected.response.json`.

8. **Read the chips and "This round".** Under "What the map reads now", one chip a reference: "Step
   1: now Heat vent (<x>, <y>), <N> ms - nearest by travel from the commander, read when the step
   starts", and for the Generator "... - the feature its beacon was placed to cover, bound when it
   deploys". Under "This round": "This round: step 1 places a new beacon near (<x>, <y>), $ 60."
   - Proof: `crates/client-gdext/tests/targeting.rs` `resolve_refs_answer_is_one_chip_per_reference`;
     `crates/gateway/tests/targeting.rs`
     `the_lull_says_what_this_round_will_do_and_round_two_finds_no_vent` (round 1's sentence ends
     ", $ 60."). The chip's words on screen: by hand only.

9. **Click a vent, then Alt-click it.** Click the starting heat vent on the map.
   - See: a menu with "Place beacon covering this vent". Close it. Alt-click the same vent: "Place
     beacon covering the nearest vent you can cover". Choose it, read the new step it **adds** at the
     route's end, and press "Undo". Alt-click adds a step with the description; it does not rewrite
     a named vent already in the route (see Deviations).
   - Proof: `crates/client-gdext/src/editor.rs`
     `a_vent_click_places_a_beacon_covering_it_and_alt_click_describes_it`; `godot_project.rs`
     `a_vent_click_offers_a_beacon_covering_it_and_alt_click_the_nearest_you_can_cover`.

10. **Every click lands the first time (F1).** From here to the end, count your clicks on Submit,
    Fix, Save and the map menus. A second click needed for one action is F1 again; note which
    button. `fixc` reproduced F1 only in part and left the rest to this check (item 129 (3)).
    - Proof: `watch_check.gd` `_first_click_lands` (Submit, Fix and Save, one click each, after a
      redraw, a menu and a scroll); `godot_project.rs`
      `the_first_click_on_the_panel_reaches_its_handler`. Your count: by hand only.

11. **Press "Submit".**
    - See: "Submitting...", then "Submitted. This is now your sealed order." and "Checked at
      submission - Ready to seal."
    - Proof: `crates/gateway/tests/methods.rs` `the_specs_fourteen_call_walkthrough_runs_end_to_end`.

12. **Press "Save" with no file yet (F6).**
    - See: the "Save as..." dialog opens at once. Save as `round1.jsonc`: "Saved to <path>."
    - Proof: `watch_check.gd` `_one_click` fails with "Save with no file did not open Save as..."
      otherwise; `godot/scripts/editor.gd` `_save`.

13. **Press "Ready".** Easy is always ready, so the Lull ends at once.
    - See: "all ready", then "Round 1 - PUSH  speed 1x".
    - Proof: `rig.rs` `a_lull_reports_the_clock_and_ends_when_every_seat_is_ready`.

## Round 1's Push

14. **Walk-in, the charge, the beacon, the Generator.** Watch at 1× or 2×.
    - See: the commander walks to the site the description chose and only then deploys. The meter's
      treasury drops by $ 60 as the deploy starts (X-12), before "beacon_placed  A beacon of seat 0
      was deployed." Then "structure_queued" (the Generator's $ 80 leaves the treasury) and
      "structure_completed  A structure of seat 0 is finished.", and supply rises from 10 kW by the
      lean vent's 20 kW. No event names the charge: the meter shows it.
    - Proof: `scenarios/s1/cover-nearest-vent.scenario.jsonc` (`beacon_placed` and
      `structure_queued` by tick 800, the beacon landing at tick 721, and its chain);
      `crates/sim/tests/deploy.rs` `a_deploy_charges_the_beacon_when_it_starts`;
      `crates/sim/tests/targeting.rs` `the_walk_in_times_out_with_no_path`;
      `crates/sim/tests/economy.rs` `only_one_generator_per_vent_adds_to_the_supply`. The meter's
      numbers: by hand only.

15. **What the demo does not show this round, and what proves it.**
    - A deploy that cannot finish refunds its $ 60 in full: `deploy.rs`
      `an_aborted_deploy_refunds_the_beacon_in_full` (the commander's death, a Push that ends) and
      `a_site_made_illegal_before_placement_aborts_and_refunds_the_deploy`. The commander leaving
      the site fails the step `out_of_range` in the code, and no test drives that case.
    - A deploy the treasury cannot cover fails its step `unaffordable` and its `on_fail` is taken:
      `scenarios/s1/unaffordable-deploy.scenario.jsonc` and `deploy.rs`
      `a_deploy_the_treasury_cannot_cover_fails_its_step`.
    - Build targets in their order and nothing inside a protected area: `crates/sim/tests/build.rs`
      `targets_build_in_their_order`, `nothing_is_built_inside_a_protected_area` and
      `a_target_one_column_outside_a_protected_box_is_built`. Hold & Build has one target and no
      protected area.
    - Work-in-hand fabrication: "unit_fabricated  A fabricator of seat 0 produced a unit." may
      appear for a build drone while the list has work. No test asserts that the fabricator makes
      build drones for a backlog and stops once it is covered: by hand only.
    - Your ore: "ore_delivered  A mining drone of seat 0 delivered ore worth $ <N>." from the top
      layer of your starting seam, which your core's default settings dig (depth 0): 24 voxels, worth
      $ 64 and $ 32, all delivered this round (`crates/sim/tests/economy.rs`'s F2 figures; S1's plan,
      task `fixs`). At depth 0 nothing is left to deliver in round 2.

16. **The recap.**
    - See: "Round 1 - RECAP" and "Recap of round 1: Round 1 ran 3600 ticks. The match continues. The
      Ledger settled and credited you $ <N>, your Basic Minimum Income at band <1 or 2> (<-5 or
      +10> %). The award fund pays out from S4, when there are awards to split it by." Press
      "Continue".
    - Proof: `crates/gateway/tests/economy.rs`
      `the_recap_names_the_seats_own_settlement_and_shortfall_and_nobody_elses`; `crates/gateway/src/strings.rs`'s
      `the_settlement_and_shortfall_lines_name_the_seats_own_figures`; `godot_project.rs`
      `the_lobby_draws_the_bridges_countdown_and_the_gateways_recap`.

## Round 2's Lull: no vent you can cover, the dormant checkbox, the load

17. **The carried draft says what this round will do (F4's cure).**
    - See: "Round 2 - LULL  5:00", "Last round's orders are loaded again and checked against the new
      map (carried from round 1)." and, under "This round", "This round: step 1 finds no vent you can
      cover." The carried description does not place again at round 1's site.
    - Proof: `crates/gateway/tests/targeting.rs`
      `the_lull_says_what_this_round_will_do_and_round_two_finds_no_vent`; `editor.rs`
      `every_lull_and_every_seal_reads_the_this_round_sentence`; `scenarios/s1/cover-nearest-vent`
      (round 2's step fails `illegal_site`, pinned by `crates/sim/tests/targeting.rs`
      `a_carried_covering_step_has_no_legal_site_after_round_one_on_the_golden_seed`).

18. **Optional: W0706.** Click the ground inside your new beacon's sphere beside the covered vent and
    choose "Place beacon".
    - See: a row "Warning", "A fixed site puts its beacon in the same place every time. A description
      such as "the nearest vent you do not cover" reads the map again each round.", "W0706 at
      /declarative/route/<n>/place_beacon/at/voxel". The row shows the diagnostic's sentence for a
      beginner (`crates/verifier/src/strings.rs`); its message ("every vent a beacon at this fixed
      site could cover is already covered: ...") is in the golden, not on screen. Its suggestion, "Cover the nearest vent you do not cover yet", is not drawn as a
      Fix button (see Deviations). Press "Undo".
    - Proof: `tests/golden/verifier/w0706_a_fixed_site_by_a_covered_vent/expected.report.json`.
      W0704 and W0705, the other two targeting lints, have goldens beside it
      (`w0704_a_lost_feature`, `w0705_an_earlier_step_places_a_beacon`) and no Fix; neither is
      produced on screen in this demo (see Deviations). W0706 on screen: by hand only.

19. **E0601 and the checkbox.** Press "Load..." and choose `round2-e0601.jsonc`.
    - See: "Opened." A row "Error", "There is not enough power for everything this playbook switches
      on.", "E0601 at /declarative/route/1", and the checkbox "Allow dormant beacons: let this route
      draw more power than you supply, so some beacons go dark." "Quick check - Cannot be sealed
      yet." Press "Submit": "Not submitted: the check found errors." The Submit button stays enabled;
      the gateway refuses the seal (see Deviations). Tick the checkbox: "Allowing dormant
      beacons...", "Dormant beacons allowed.", and E0601 becomes W0603, "Warning", "Some of what this playbook
      switches on will have no power and will sit idle." A W0602 row ("This playbook
      spends more than the seat is likely to have.") shows only if the file's committed spending
      outruns your projected treasury, which this sheet cannot promise (see Deviations).
    - Proof: `editor.rs` `the_dormant_checkbox_is_the_verifiers_patch_and_its_removal`;
      `tests/golden/verifier/e0601_a_scout_count_past_every_supply`, `w0603_a_shortfall_allowed` and
      `w0602_spending_outruns_the_treasury`. The recipe was checked under `gamectl verify`'s
      reference view only (E0601 for 34 kW beyond supply, W0602), where it also gives E0403 (the
      Survey site lies outside every sphere of the reference seat) and W0704 (no uncovered vent is
      live there); in the match, round 1's covering beacon puts the site inside a sphere. In the
      match: by hand only.

20. **Load the round's real orders.** Press "Load..." and choose `round2-brownout.jsonc`, then
    "Submit", "Save as..." to keep it, and "Ready".
    - See: "This round: step 1 finds no vent you can cover." again, since the file's step 1 covers
      the nearest vent you do not cover yet. The file places a low-priority Survey beacon with seven
      scouts at (364, 30, 36), waits until headroom is at most 18 kW, walks to (338, 20, 36) and
      recycles the nearest Build beacon, which is the one round 1 placed.
    - Proof: `scenarios/s1/brownout-by-recycle.scenario.jsonc` plays this file for seat 0 on the same
      seed, with seat 1 on the gateway's safe playbook. A review of this sheet ran it again as a
      scratch probe, not a committed scenario, with seat 1 `builtin` (Easy), as in the lobby: its
      four event assertions held (`beacon_placed` by tick 800, `beacon_recycled` by 4911,
      `beacon_browned_out` by 4912, `segment_ended` by 7200). The scenario's round 2 is 180 000 ms
      against the lobby's 300 000 ms; the recycle lands about 65 s into it, so the longer Push does
      not change it. The scenario's round 1 plays this file, not Hold & Build; that difference was
      not probed. `gamectl verify` on the committed file gives E0403 and W0704 under its reference
      view, as for step 19's copy; in the match, round 1's covering beacon puts the site inside a
      sphere.

## Round 2's Push: the recycle and the brownout

21. **The load, the recycle, the deficit.** No "ore_delivered" row for seat 0 this round: the top
    layer went in round 1, and the file writes no dig depth (see Deviations).
    - See: the Survey beacon lands, "A fabricator of seat 0 produced a unit." for each scout, then
      "beacon_recycled  A beacon of seat 0 was recycled on site for $ <N>.", half the beacon's
      remaining value (build cost × HP; $ 30 at full HP), with "beacon_destroyed" after it. The
      Generator homed to it is ruined, supply falls back to the core's, and at the next settle
      "beacon_browned_out  A beacon of seat 0 went dark: draw outran supply." for the Survey beacon,
      the lowest priority. The core stays lit, and the commander walks on through the blackout.
    - Proof: the brownout scenario asserts `beacon_recycled` by tick 4911 and `beacon_browned_out`
      by tick 4912 for seat 0, with its chain; `economy.rs`
      `value_follows_condition_at_the_audit_and_at_the_recycle_refund`,
      `the_brownout_order_is_priority_then_distance_then_the_core_last` and
      `the_commander_walks_through_a_blackout_drawing_nothing`.
    - Not shown here, and proved only by tests: the Quartermaster holding fabricator orders first
      (`a_fabricator_order_the_headroom_cannot_run_is_held_until_a_generator_brings_supply_back`)
      and an idle expansion kept lit (`a_beacon_whose_shed_relieves_nothing_is_skipped_and_stays_lit`,
      `a_beacon_whose_shed_would_deepen_the_deficit_is_skipped`); the scenario has neither an order
      held nor an idle expansion (see Deviations).

22. **The recap.**
    - See: the settlement line, then "When the segment ended, <b_NN> was dark: draw outran supply,
      and the brownout shed <N> kW.", and the step that found no site: "A step failed
      (illegal_site): it found no legal site: what it matched had no site the seat may place on
      within reach." Section 1.1's "3 matched, none reachable" is the line for a description whose
      matches no route reaches, which this match does not make (see Deviations). Press "Continue".
    - Proof: `crates/gateway/tests/economy.rs` `the_shortfalls_kw_is_the_grids_shed_read_at_segment_end`;
      `crates/gateway/src/strings.rs`'s `the_recaps_step_failure_says_how_much_matched_and_keeps_the_feeds_words_otherwise`
      (which asserts "A step found nothing: 3 matched, none reachable.").

## Round 3's Lull: a raise, and a timeout

23. **Optional: raise the dark beacon.** Click the dark Survey beacon: "Beacon <b_NN>", then "Visit &
    change", "Set priority high", and read the new step.
    - Expected: with the core the only lit beacon, and the core outranking every knob, no lower
      shed covers the Survey beacon's load, so the raise relights nothing in this match. Section
      1.1's relight at once needs a lit beacon of lower priority whose shed covers the load. This
      reading is from the code, not from a run.
    - Proof: `economy.rs` `a_raise_relights_a_dark_beacon_when_a_lower_shed_covers_it` and
      `a_raise_relights_nothing_when_no_lower_shed_covers_it`. Press "Undo" before step 24.

24. **Let the Lull run out (F5).** Press nothing: no Submit, no Ready. Let the timer reach 0:00.
    - See: "Round 3 - PUSH", and in the event list "safe_playbook_filed  The Lull ran out, so the safe
      playbook was filed for you." The host files the built-in operator's safe playbook for your
      seat, which walks to the safest beacon and ends at the core with a step that gives its seam a
      dig depth of 4 (`deepen_core`).
    - Proof: `crates/gateway/tests/demo_findings.rs`
      `a_timeout_filing_is_named_in_the_seats_own_feed`; `crates/operator/src/safe.rs` module doc,
      "The core's dig depth". No client test covers the row.

## Round 3's Push and the end

25. **The mining drone keeps delivering (F2's cure).** Use "4x" or "Skip" if you like.
    - See: "ore_delivered" rows for seat 0 again in round 3, after none in round 2. The top layer was
      spent in round 1, so these come from below the rim: the core's depth 4 reaches the whole
      four-voxel seam, dug around pillars at their spacing. A spent seam does not read as spent on screen (see Deviations).
    - Proof: `scenarios/s1/mine-to-depth.scenario.jsonc` (round 1's delivery asserted; rounds 2 and
      3 pinned by its chain); `economy.rs` `a_starting_seam_yields_more_than_its_exposed_rim`;
      `crates/sim/tests/mining.rs` `digging_stops_at_the_max_depth`,
      `pillars_stand_at_their_spacing`, `a_held_seam_is_kept_until_spent`. The amounts on screen: by
      hand only.

26. **The last recap and the end.** Press "Continue" at the recap.
    - See: "Round 3 - ENDED  The last match has ended, so there is nothing to resume.", the event
      list's "match_ended  The match ended: seat <N> won." (only "The match ended." on a shared win),
      and the recap "... The match ended: round_limit. The final audit names seat <N> the winner." An
      exact tie on every term reads "... ties seats 0 and 1 on every term: a shared win." The fog
      lifts: Easy's beacons appear. Clicking one says "That beacon is not yours." and names no
      `e_NN` (see Deviations).
    - Proof: `crates/sim/tests/audit.rs` `a_round_limit_end_names_the_final_audits_winner`,
      `a_net_worth_tie_goes_to_fewer_beacons_lost`, `an_exact_tie_on_every_term_is_a_shared_win`;
      `crates/sim/tests/runner.rs` `the_round_limit_is_decided_at_the_last_rounds_final_tick`;
      `scenarios/s1/round-limit-audit.scenario.jsonc` (the winner pinned by its chain); `crates/gateway/src/strings.rs`'s
      `the_recap_says_whether_the_match_is_over`; `crates/gateway/tests/view.rs`
      `another_seats_beacon_is_e_nn_in_first_sighting_order` for the wire's `e_NN`.

## Beside the demo

### P1, the certifying run (S1's plan, section 5)

**The run.** The owner's machine (Intel i7-9800X, Windows 10 Pro), `main` at `49398cf`,
2026-10-12, three runs of

```sh
cargo test --release --locked -p pharmakos-bench --test perf_alarm -- --ignored --nocapture
```

The raw log is `D:/build/s1-demo/p1-run.log`. Run 1's notices, as printed:

```text
::notice title=QUICK p99::1541.900 us p99, 35.600 us p50 on windows, over 72 committed verifier cases x 200 calls each, timed singly (Input::new then verify at QUICK); spec section 16's P1 bar is 5 ms p99, published here and never gated
::notice title=QUICK p99 at the size budget::budget_128: 1818.600 us p99, 1604.200 us p50 on windows over 200 calls; slowest case by p99: budget_128 at 1818.600 us
::notice title=decision tick per size unit::94 ns per size unit per decision tick on windows (3 seats x 128 units sealed; a decision tick 85.771 us against an ordinary tick 49.399 us, mean excess 36386 ns over 720 windows of 3600 ticks, fastest run per tick); a whole 50 ms tick with 3 seats would hold 177304 units per seat, a ceiling and not a budget
```

| Figure | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| QUICK p99, 72 committed cases × 200 calls | 1541.9 µs | 1614.7 µs | 1683.1 µs |
| QUICK p50, the same | 35.6 µs | 38.4 µs | 38.4 µs |
| QUICK p99 at `budget_128` | 1818.6 µs | 1899.4 µs | 1875.0 µs |
| QUICK p50 at `budget_128` | 1604.2 µs | 1668.2 µs | 1719.0 µs |
| ns per size unit per decision tick | 94 | 93 | 94 |
| decision tick / ordinary tick | 85.8 / 49.4 µs | 85.4 / 49.4 µs | 86.0 / 49.7 µs |
| units per seat a 50 ms tick would hold (3 seats) | 177 304 | 179 211 | 177 304 |

**The CI runners, beside it.** The `perf alarms (<os>)` jobs of `main`'s push run for `49398cf`
(run 38168063262) published, as `::notice::` annotations, never gated and never compared across
operating systems:

| Runner | QUICK p99 / p50, 72 cases | `budget_128` p99 / p50 | ns per size unit per decision tick | units per seat at a 50 ms tick |
|---|---|---|---|---|
| `windows-latest` | 1280.3 / 25.6 µs | 1366.2 / 1302.1 µs | 79 | 210 970 |
| `macos-latest` | 751.5 / 18.0 µs | 1134.6 / 791.2 µs | 71 | 234 741 |
| `ubuntu-latest` | 508.2 / 11.3 µs | 539.2 / 511.7 µs | 55 | 303 030 |

On macOS the slowest case by p99 was `e0109_negative_duration` (1798.0 µs), not `budget_128`.

**The verdict.** QUICK ≤ 5 ms p99 is met at the size budget with about 2.6× headroom (1899.4 µs,
the worst of the three), so no fallback is taken. Byte-identical reports across the operating
systems are held by the verifier goldens on every pull request. The per-rule cost is 93–94 ns per
size unit per decision tick (three seats of 128 units), the input S3 sets the size budget from;
the 177–179 k units a seat is the bench's ceiling at a 50 ms tick, not a budget.
`verifier.size_budget_units` stays 128 until S3's exit.

**A gap: the counted units.** S1's plan section 5 item 3 asks for the per-rule cost in counted
evaluation units per decision tick per seat as well as in nanoseconds. The sim counts them
(`World::decision_work`, the unhashed counter `tgt` added), but no test or notice prints that figure
at the size budget: `crates/sim/tests/targeting.rs` `the_decision_tick_counts_its_evaluation_units`
pins small fixtures only (3 and 4 units for a handler, 423 for one covering resolution on the golden
seed), and `pharmakos-bench` publishes nanoseconds alone. The count per decision tick per seat at
128 size units is not recorded here; S3 sets the size budget from the nanosecond figure unless a
lane adds the count to the bench's notice. The map is `BENCH_SEED`
(`0x00000000ca5caded`, S1-49).

**Not measured: FULL against a late-match snapshot.** Item 133 carried this to the P1 run: FULL
decodes the whole snapshot on every call for `coming_segment_ms`, and moving the length into `Scope`
is a `report_hash` contract change and a gateway change. `pharmakos-bench` times QUICK only, so
there is no FULL figure from this run or any other. The choice is the owner's (ruling 8 below).

### The 10 000-playbook fuzz

`nightly-scenarios.yml`, forced (`force`) on `main` at `49398cf`, 2026-10-12, run 38244607560. The
`verifier fuzz (10 000 playbooks)` job passed: "test ten_thousand_playbooks_come_back_as_reports ...
ok", in 9.92 s, no panic (`crates/verifier/tests/fuzz.rs`).

The three adversarial jobs, `hunter vs Balanced`, `rusher vs Balanced` and `turtle vs Balanced`,
failed at once with "gamectl: invalid option '--operator'. `gamectl --help` lists the options."
That is expected: they are S2's (AGENTS.md section 10 item 5, `NIGHTLY_SCENARIOS_ENABLED` unset),
`gamectl scenario run` takes no `--operator` until then (`crates/gamectl/src/cli.rs` `OPTIONS`; a
`builtin` seat is always Easy), and the register's S2-01 already names the flags. It is S2's carry.

### The balance check's report

`tune` #101 ran `crates/gamectl/tests/balance.rs`'s 80 matches on the finished rules (rules hash
`b1f2eef0e75c5ae4`) and put the report's header, summary and `expand_east` column in its pull
request body; the full report (232 KB) regenerates byte for byte with
`cargo test --release -p pharmakos-gamectl --test balance -- --ignored --nocapture`. Its findings
are item 137 (2) (e) to (g): no seat fails to afford a Generator (0 of 160) and every seat builds
one; ONLY-GROWS flags 130 of 160 seat-matches and STALLED 160 of 160, both from Easy's reach (it
seals its safe playbook from round 2, with no uncovered feature in one beacon's reach) rather than
from a price; `expand_east` cannot reach its first site in 120 s at any speed in spec section 4's
range; and the starting seam is dry by round 4 or 5 at any depth, the seam being finite.

## Owner rulings at the demo

Each is open until the owner rules; each carries the main session's recommendation and the
downsides. "Engineering" calls were decided under the standing instruction and are listed only for
the owner's veto.

1. **Item 137 (2) (d), `DETERMINISM_TICKS`.** Recommended: keep 3 600, the initial state and the
   first Push's first 3 599 ticks. *Downside:* the determinism chain misses the Push's closing tick
   (the segment end, the recap's opening, the respawn settlement), which only the scenario chains
   cover. *The other option,* 3 601: one more line and a re-bless; its downside is a re-bless for a
   tick the scenario chains already pin.
2. **Item 137 (2) (e), decision 16 read as "no value moves".** Recommended: accept. *Downside:*
   S1's economy is numbered by a check Easy cannot exercise past round 1, and the reading departs
   from decision 16's letter (a flagged row gets a proposed change). *The other option,* a probe with
   a moved price: it cannot clear flags that come from a playbook that buys nothing.
3. **Item 137 (2) (f), `expand_east`.** Recommended: move the example, giving the spec's worked
   example and `scenarios/skeleton/expand-east-segment` a timeout its walk can meet, or a nearer
   first site. *Downside:* it re-blesses `expand-east-segment`'s chain and event log, and changes the spec's
   worked example and the scenario's stated purpose (the illustration of a timeout no longer shows
   a tight one); the tension between walk speed and map span (item 103 (3)) stays for S4's map
   re-derivation. *The
   other options:* the speed cannot fit inside spec section 4's 10–15 cost/s (it would need 25 and
   41); shrinking the map moves every chain and the mapgen golden for a stage that re-derives the
   map anyway.
4. **Item 137 (2) (g), the core's dig depth and the finite seam (S1-52).** Recommended: keep
   `CORE_DIG_MAX_DEPTH` at 4 (it reaches the whole four-voxel seam) and `economy.seam_voxels` at 150
   for S1. *Downside:* a six-round match's ore runs dry by round 4 or 5; the lobby's three rounds do
   not reach it. *The other option,* more ore (`seam_voxels`, or a second seam, which waits for S3's
   `toward`): a rules change that moves the rules hash and every chain.
5. **The balance check's seed set and its flag readings (S1-53, S1-54)** (`crates/gamectl/tests/balance.rs`, two
   markers). Recommended: accept the twenty seeds (the two golden seeds and eighteen spread by the
   64-bit golden ratio) and the three readings as written (a flat treasury counts as ONLY-GROWS; any
   `beacon_placed` counts as a new asset for STALLED; NEVER-AFFORDS-GENERATOR counts round 1's
   opening treasury). *Downside:* the readings are guesses no decision fixes, and the STALLED
   comment's note that a free stacked beacon hides a stall predates X-12 and no stacking.
6. **Every S1 economy row's PLACEHOLDER** (`rules/README.md`'s rows marked S1, kept at item 90's
   seed, `economy.mining_carry_voxels` and `structures.build_hp_per_second` at revision 4's value),
   **the two Lull rows** (`match.lull_ms` 300 000 and `match.first_lull_ms` 600 000) **and the
   skeleton's rows that stood at its demo** (D-02 to D-19: the commander, the sphere, the map and
   spawn rows, `structures.beacon`). Recommended: every value stands for S1, the economy rows
   re-checked when Easy can reach past one beacon (S3's `toward`). *Downside:* S1 ends numbered by
   an economy nobody exercised past round 1. A changed value moves the rules hash and every
   `report_hash` golden, and a change to a value the sim reads moves the chains.
7. **S1-37, the fielding limits, and S1-49, `BENCH_SEED`.** Recommended: keep the totals (40
   beacons, 300 units, 120 structures a match, shared equally per seat), revisited at S2's
   G3′-real as already planned; and ratify `0x00000000ca5caded`, the committed scenarios' seed, as
   P1's map. *Downside:* the check never approached the limits (the most one seat reached: 1 beacon
   placed, 1 structure, 1 unit fabricated), so they are unmeasured as limits; one seed for P1's
   per-unit figure.
8. **P1's FULL budget, and whether the coming segment's length moves into `Scope` (item 133).**
   Two options:
   - **(a)** A small bench lane before S1 closes: `pharmakos-bench` times FULL against a late-match
     snapshot, so the `Scope` decision is made with data. *Downside:* a lane, an `xtask`-adjacent
     walled crate change and its reviews before S1 can close, for a budget spec section 16 does not
     gate until S3.
   - **(b) (Recommended)** Leave FULL and its budget to S3's exit, where spec section 16 gates "FULL
     ≤50 ms p99 at the size budget", and measure it there with the `Scope` question. *Downside:* the
     `Scope` decision, a `report_hash` contract change, waits without a measurement, and if FULL's
     snapshot decode proves slow, S3 carries the contract change alongside its own work. It
     also leaves undone what item 133 and S1's plan's `demo` section assigned to this run, a FULL
     measurement against a late-match snapshot, so the owner's ruling, not the plan, moves it.
9. **Who the final audit decides between** (`crates/sim/src/audit.rs`, "owner, at S1's stage
   demo"; X-08). Recommended: confirm the reading, the seats still in the match (on a no-survivor
   end, the seats that fell on the latest elimination tick). *Downside:* the spec does not spell it
   out, and a seat eliminated earlier can never win on held value it once had.
10. **The half-written Build list (item 136 (5), for S2).** A settings row that can write only part
    of a Build target list drops entries silently today (`World::replace_build_targets`). The spec
    does not state the rule (AGENTS.md section 12). Recommended, to rule now or with S2's first
    `crates/sim` lane: the row is atomic, as interface rows are; it writes the whole list or nothing
    and fails its step with a new, additive `StepFailure` id. *Downside:* a long list loses every
    entry, those that would have fitted included. *The other option,* write what fits and report the
    dropped tail: a partial state the player did not write, which an event must explain.
11. **F3 (item 129 (2) (c)).** The owner's word was to build the guard only if F3 recurs in the next
    round of testing. Recommended: if step 2 showed no hang, F3 closes with no guard. *Downside:* two
    instances keep sharing `user://` (the log, the shader caches, `last_match.txt`).
12. **The deviations below.** Recommended: accept each, with the carries named.
13. **The delegation.** CLAUDE.md returns it to the owner at S1's stage demo (items 85, 86, 116 (2)
    and (9), 120, renewed by item 126 (2) (g)). Recommended: renew it for S2 on the same terms,
    the owner reviewing S2's stage demo rather than each pull request. *Downside:* the owner keeps
    reviewing at stage granularity, with the AI reviews standing in for a second human reviewer,
    which they are not; ending it puts every merge, contract pull requests included, back in the
    owner's hands.
    - *Ruled 2026-10-14 (item 141):* renewed for S2 on the same terms, effective at once, with no
      gap between S1's stage demo and S1's close; at S2's stage demo the owner says whether it runs
      on into S3.

14. **The main session's calls under the standing instruction, items 129 to 137.** Handoff NEXT 3
    lists them: among others item 137 (2) (a) to (j), item 136 (3)'s calls, item 135 (2)'s, item
    134 (2)'s four, item 133 (3)'s ten, item 131 (4)'s six and item 130 (3) (a) and (b), the S3
    re-stagings. Two of the last still say so in the proto: `gateway.proto`'s "for the owner to
    confirm" on `GetMapSummaryResponse`'s held terrain summary (~765, S3-23) and on
    `GetEconomyForecastResponse`'s projection and projected income (~868, S1-46). Recommended:
    ratify them as a set, the two markers reworded by the next `proto/**` pull request. *Downside:*
    anything the owner later disagrees with becomes a change, with its own pull request and, where a
    call moved a chain or a golden, its re-bless.
15. **A minimum beacon spacing (S1's plan, decision 10).** S1 rules "no stacking" only: two
    beacons may not share a column, and nothing else spaces them. Recommended: keep it, no spacing
    row. *Downside:* a seat may crowd beacons side by side, and a larger spacing wanted later is a
    new rules row in a contract pull request that moves every chain.

## Deviations from section 1.1

Each sentence of section 1.1 was checked against the code and the tests at `49398cf`. Where the
build does what the section says, the steps above cite the proof; these are the places it does
not, or does it differently.

1. **F3: no second-instance message.** Section 1.1: "A second `Pharmakos.exe` started beside it says
   it is already running rather than hanging". There is no single-instance guard and no such string
   in `godot/`, `crates/client-gdext` or `crates/gamectl`; the owner deferred the guard until F3
   recurs (item 129 (2) (c), amending decision 19), and the flushed log is what shipped (step 2).
2. **The wizard suggests a name, not the description.** The template's default is the description,
   but Easy's suggestion, which the page shows first, is `{"feature_id":"vent_324_16"}`; the demo
   types the description back (step 5).
3. **Step 1's words.** Section 1.1: "place a beacon covering the nearest vent you can cover". The
   rendered step reads "place a beacon at a site covering the nearest heat vent you do not cover
   yet (nearest by travel, read when the step starts)" (step 7). "The nearest vent you can cover" is
   the Alt-click menu's wording.
4. **The chip's form.** Section 1.1: "now: Heat vent (x, y) · N s — nearest by travel from the
   commander, read when the step starts". The chip reads "Step 1: now Heat vent (x, y), N ms -
   nearest by travel from the commander, read when the step starts": a step prefix, game
   milliseconds rather than seconds (S3-18), and no "·" or "—".
5. **Alt-click adds a step.** Section 1.1: "Alt-click on a vent turns a named vent into that
   description". Alt-click offers "Place beacon covering the nearest vent you can cover", which adds
   a new `place_beacon` step with the description; it does not rewrite a named vent already in the
   route (`editor.gd` `_on_vent_clicked`; step 9).
6. **E0601 does not disable Submit.** Section 1.1: "blocks Submit until `allow_dormant_beacons` is
   set". The button stays enabled; the gateway refuses the seal and the editor says "Not submitted:
   the check found errors." The option is a checkbox, because the verifier marks its patch
   maybe-incorrect (step 19).
7. **No targeting lint has a Fix button.** Section 1.1: "the three targeting lints with their Fix".
   W0704 and W0705 carry no suggestion; W0706's, "Cover the nearest vent you do not cover yet", is
   `MaybeIncorrect` (`crates/verifier/src/lint.rs`), and the editor draws only machine-applicable
   fixes as buttons (`editor.rs` `rows_of`). The only targeting diagnostic with a Fix button is
   E0411's "Leave the coverage out". W0704 and W0705 are not produced on screen in this demo at
   all: no step of the sheet's playbooks names a lost feature in the match or reads a description
   after an earlier placing step, so their goldens are the only proof.
8. **The deficit sheds a Survey beacon; the fabricator hold and the idle expansion are tests only.**
   Section 1.1: a deficit "holds fabricator orders first, then darkens domes in the brownout order
   with an idle expansion kept lit". The committed demo of a deficit, `brownout-by-recycle`, sheds
   its low-priority Survey beacon; it has no fabricator order held and no idle expansion. Both rules
   are in the code (`SingleTreasury`'s headroom refusal; `power.rs` `shed_if_it_relieves`) and
   proved by `economy.rs` tests (step 21).
9. **The relight is not reachable in this match.** Section 1.1: "a priority raise on site relights a
   dark beacon at once". It does when a lit beacon of lower rank can be shed to cover the load; with
   the core the only lit beacon, and the core outranking every knob, nothing relights (step 23).
   Proved by `a_raise_relights_a_dark_beacon_when_a_lower_shed_covers_it`.
10. **"3 matched, none reachable" is not in this match's recap.** The line exists and is tested
    (step 22), but it is said for a `no_target` whose description matched features no route
    reaches. On the golden seed round 2's carried step fails `illegal_site`, which reads "A step
    failed (illegal_site): it found no legal site: ...".
11. **A spent seam does not read as spent.** No event or view field says a seam is spent. A beacon's
    held seam is released and re-chosen when spent (`crates/sim/src/mining.rs` `refresh_seams`). On
    the wire a seam reads as lost (`live` false in `get_map_summary`) only when no ore is left at any
    depth (`crates/sim/src/features.rs` `FeatureTable::is_live`), so a seam dug to its limit around
    its pillars stays live; and the client passes `live` to the editor's vent pick and draws no seam
    state at all.
12. **Enemy beacons are not shown as `e_NN`.** The gateway names them so on the wire
    (`crates/gateway/src/viewfeed.rs`, tested by `another_seats_beacon_is_e_nn_in_first_sighting_order`),
    but the client never draws a beacon's id for another seat: the vista uses it as a hidden node
    name, a click says "That beacon is not yours.", and the event list says "A beacon of seat N".
13. **The Build list's order is its `order` field.** Section 1.1: "targets go up in their written
    order". They go up by each target's `order`, the written position breaking ties, and an
    affordable target is asked for before a higher-order one the treasury cannot cover (`mandate.rs`
    `next_unpaid_target`; `build.rs` `targets_build_in_their_order`,
    `an_affordable_target_is_asked_for_before_a_higher_order_one_that_is_not`).
14. **No test pins work-in-hand fabrication.** The rule is in `mandate.rs` (`backlogged`,
    `Build::request`); no sim, gateway or scenario test asserts that the fabricator makes build
    drones for a backlog and stops once covered (step 15).
15. **The refunds and the charge have no event.** Section 1.1's "$ 60 leaves the treasury as the
    deploy starts" and the full refund are visible only in the meter's treasury; `deploy.rs` says
    "No event carries a charge". The commander leaving mid-deploy fails the step `out_of_range` and
    refunds, in code, and no test drives that case (step 15).
16. **F2's cure needs a written depth, and round 2 does not "keep delivering".** The starting drone
    digs below the rim only when the seat's playbook writes a dig depth (item 133 (3) (a)). At depth
    0 seat 0's top layer, 24 voxels ($ 64 + $ 32), is all delivered in round 1, so round 2, whose
    brownout file writes no depth, shows no `ore_delivered` for seat 0: section 1.1's "keeps
    delivering in rounds 2 and 3" is round 3 only here. The demo shows the cure through the safe
    playbook a timeout files (steps 21, 24 and 25); `mine-to-depth` pins deliveries in every round
    for a seat that writes a depth.
17. **W0602 is not guaranteed on screen.** Section 1.1: the editor shows "W0602 when committed
    spending outruns the projected treasury". It does, but whether step 19's file outruns the
    treasury depends on what the match has left in it; the golden
    `w0602_spending_outruns_the_treasury` is the proof that holds.

Smaller differences, each already decided elsewhere:

- Travel times are game milliseconds (S3-18), and the placement ghost answers per click (S3-02).
- Section 1.1's demo of a deficit needs a playbook the zip does not ship; the sheet loads the
  committed scenario's file from a checkout (step 20).
- Easy seals its safe playbook from round 2 on, because nothing it can reach is uncovered (item 136
  (3) (q)); you cannot see its seat until the end.

## Found while writing this sheet

Not deviations; each is a stale comment or a carry for its next owner.

- `.github/workflows/release.yml` reads its notes from the hard-coded `0.1.0-dev+skeleton` heading,
  so no S1 tag was cut: the next `.github/**` pull request.
- `scenarios/s1/unaffordable-deploy.scenario.jsonc` still says the `$` projection that would warn
  about its fourth deploy "is S1's economy work, not yet built"; `proj` #85 built it. The next lane
  that touches `scenarios/s1/`.
- `proto/gp/v1/rules.proto`'s `Match.lull_ms` comment still says the gateway and `gamectl` time every
  Lull with that row until they read `first_lull_ms`, which `econ` #92 did; and its
  `mining_carry_voxels` and `build_hp_per_second` comments still speak of a lane swapping a constant.
  The next `proto/**` pull request, with S1-42's row.
- `crates/sim/src/economy.rs` expands BMI as "Base Maintenance Indemnity", and the events and the
  gateway's strings as "Basic Minimum Income": the next `crates/sim` lane.
- Expand & Mine's wizard page says "ore seam" and its rendered text "scrap seam": the next owner of
  `library/`.
- `scenarios/README.md` (~161) still calls the determinism harness's 50 walkers "a PLACEHOLDER for
  the owner at S1's demo"; `tune` #101 deleted them and S2-16 now holds the walker cap. The next lane
  that touches `scenarios/`.
