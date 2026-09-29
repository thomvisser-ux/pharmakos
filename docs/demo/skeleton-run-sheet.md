<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Stage demo run sheet: the walking skeleton

The owner's review of the stage demo ends the walking skeleton (AGENTS.md section 10 item 8). This
sheet walks the demo of skeleton-plan section 1.1 as the zip plays it, one step at a time: what to
click, what to see, and which test or scenario also proves the step, or "by hand only" where none
does.

It is written against `main` at `640d5b9` plus the changes T22a makes beside it, which decisions-log
item 123 (2) 3, 7 and 8 decide; each place that depends on them says **(with T22a)**. Item 123 (3)
and the plan's T22b section cite "(2) 3, 5 and 8": (2) 5, the Fix button reached by a hand edit,
changes nothing in T22a and is checked here by hand (steps 22 and 26), and (2) 7 is T22a's
Quartermaster test (step 18). The words in quotation marks are the interface's own, from
`godot/scripts/strings.gd` and `godot/scripts/lobby.gd`, or the gateway's, as the client shows
them. Where the demo differs from
section 1.1, the step says so, and "Deviations from section 1.1" at the end lists each difference
for the owner's ruling. Row IDs such as D-20 are rows of the PLACEHOLDER register,
`docs/placeholders.md`.

## Before you start

**The zip.** Use `v0.1.0-dev.2`, the draft prerelease the main session cuts once T22a and T22b have
merged and `main`'s push run is green (item 123 (2) 12). Do not use `v0.1.0-dev.1`: it predates
T21b's fix to the rig's rate budget and T22a's changes (the three-round lobby and the camera that
ignores typing).

**Unzipping.**

- Windows: extract the whole zip, keep the files together, and run `Pharmakos\Pharmakos.exe`. The
  build is unsigned: if SmartScreen says "Windows protected your PC", choose "More info", then "Run
  anyway". The zip's `README.txt` says the same, and what to do if antivirus quarantines
  `gamectl.exe`.
- Linux: extract the whole zip, run `chmod +x Pharmakos.x86_64 gamectl` if the archive tool dropped
  the executable bits, then `./Pharmakos.x86_64`.
- There is no macOS zip (`cargo xtask package` refuses macOS).

**Where things are kept.**

| What | Windows | Linux |
|---|---|---|
| The private match cache: one folder per match, holding `save.json`, `match.json`, `audit.log`, the sealed playbooks and the replay's hash chains (`crates/gateway/src/cache.rs`) | `%LOCALAPPDATA%\Pharmakos\matches\<match-id>\` | `$XDG_DATA_HOME/pharmakos/matches/<match-id>/`, by default `~/.local/share/pharmakos/matches/` |
| The lobby's remembered last match, `last_match.txt`, and the game's log | `%APPDATA%\Godot\app_userdata\Pharmakos\` | `~/.local/share/godot/app_userdata/Pharmakos/` |

A lobby match is named `local-<unix seconds>-<pid>`, so the newest `local-*` folder is the match you
are playing.

**Have ready:** a plain-text editor and a folder to save playbooks in (round 1 saves one and round 2
loads an edited copy); Task Manager on Windows or a terminal on Linux, to see whether `gamectl` is
running; and about forty minutes. Every Lull lasts three minutes, and the Pushes last 3, 5 and 8
minutes at 1×; the speed buttons and Skip shorten them.

**What a headless run cannot prove about the zip** (item 121 (1)). CI's `clean launch` jobs run the
zip's own smoke check and `gamectl seat doctor` on machines with no toolchain, headless. Four things
only a person at a real machine sees, so look for each and note what you saw:

1. **Rendering.** The vista draws the terrain under the ash sky, the beacons, the units and the
   route. The entities are placeholder shapes until the art pass (S6).
2. **The GPU and Vulkan.** The game needs a Vulkan-capable GPU (Godot's Forward+ renderer); on a
   machine without one it does not start.
3. **A firewall prompt when the host binds loopback.** The host binds `127.0.0.1` on a port the OS
   picks (`crates/gateway/tests/host_loop.rs` `the_host_binds_loopback_on_an_ephemeral_port`). A
   loopback bind is not expected to raise a prompt; if one appears at New match, note it.
4. **A console window when the lobby starts `gamectl`.** The lobby spawns `gamectl host` through
   Godot's `OS.execute_with_pipe` (`godot/scripts/host_link.gd`), and whether Windows shows a console
   window for it depends on the engine, not on this repository: unverified either way. Note whether
   one appears at New match.

## The lobby

1. **Launch the game.**
   - See: the chooser, "New match" and "Credits", with the line "Start a new match, or go on with
     the last one." "Resume last match" appears only when a match is remembered, so not on a first
     launch.
   - Proof: `crates/client-gdext/tests/godot_project.rs` `the_smoke_check_drives_the_real_lobby` and
     `the_credits_button_opens_an_overlay_from_the_chooser`; the shipped smoke check in the `package`
     and `clean launch` jobs.

2. **Press "New match".**
   - See: "Starting the match host...", then "Connected. Waiting for the view...", then the vista
     frames the whole map by itself when the first view arrives, and the status line reads "Round 1
     - LULL  3:00  speed 1x", the timer counting down. The match is two seats on the golden seed
     `0x00000000ca5caded`: you at seat 0, Easy at seat 1, the rules' 3 / 5 / 8-minute ladder and a
     round limit of 3 **(with T22a)**. Check points 3 and 4 above now.
   - Proof: `godot/scripts/watch_check.gd` (the host announces, both connections open, the first
     keyframe arrives whole), run in the `client extension (<os>)` jobs; the smoke check.

3. **Look at the map before planning.** Free-look: W, A, S, D or the arrow keys pan, Q and E lower
   and raise, the mouse wheel zooms, and the right mouse button held down turns the view. "Whole map"
   only frames the camera on the map, and works at any time; it unlocks nothing.
   - See: your fortified core beacon, the commander, two build drones and a mining drone, a heat vent
     a short walk away, and the Easy seat nowhere: enemy units, beacons and voxel edits show only
     inside your own spheres until the match ends (T16a notes). The panel on the right, "Orders",
     shows "Your economy": "$200   supply 10 kW   draw 4 kW   headroom 6 kW" (two rounds of BMI;
     the core's surplus; one kW for each of the four units, the core's own base netted out by T14b,
     so not the "6 kW" item 113 (5) recorded before it). The map is about 384 × 384 × 64 voxels
     (D-08 to D-10), and Easy's spawn zone lies at least the spawn-distance rule away, about one and
     a half early Pushes' travel (D-17, D-18), far outside your spheres. Your core is on a Build
     mandate; no view shows a beacon's mandate, so only a test shows it.
   - Proof: `crates/sim/tests/mapgen.rs` `spawn_distance_holds_for_every_seed_in_the_set` (the golden
     seed is in its seed set); `crates/sim/tests/determinism.rs`
     `the_wire_ids_of_the_new_enums_are_dense_and_unique` ("every pre-placed core is on a Build
     mandate"); `crates/gateway/tests/view.rs` `a_fogged_seat_never_receives_an_unseen_entity`;
     `tests/golden/economy/expected.settlement.txt` (supply 10 and draw 4 on every row of a seat that
     built nothing), from `crates/sim/tests/economy.rs` `the_settlement_ledger_matches_its_golden`;
     `crates/gateway/tests/advice.rs` `a_seat_is_told_its_own_economy_and_never_another_seats`; the
     watch check's `_meter`. The camera, and the map's size and the distance on screen: by hand
     only.

## Round 1's Lull: the wizard, a parameter, the route, the checks, submit

Every Lull lasts 180 seconds, the rules' `match.lull_ms`, one length for every Lull (D-01). A Lull
that runs out with nothing sealed by a seat files the safe playbook for that seat, which is section
1.1's "cost of a timeout" (item 123 (2) 4). Section 1.1's Lull beats are spread over the three Lulls
so that each fits: the wizard and submit here, the carried draft, the notes box and the Fix button
in round 2, save and resume in round 3.

4. **Open the three templates.** Under "Start from a template" are "Expand & Mine", "Hold & Build"
   and "Safe playbook". Press "Expand & Mine".
   - See: "Template: Expand & Mine", one page for each of its three parameters ("Where to walk to
     before placing", "Where the Mine beacon goes: inside one of your spheres, beside a seam", "How
     deep the Mine mandate digs, in voxels"), each with its value as raw JSON, the mark "suggested by
     the built-in operator" where Easy suggested it, and "Why: ..." with Easy's sentence. Press
     "Close".
   - Proof: the watch check's `_templates` and `_template` (every template `list_templates` lists is
     drawn and driven to a submission); `crates/client-gdext/tests/wizard.rs`
     `the_pages_are_the_fixtures_parameters_as_they_came`.

5. **See what a timeout would file.** Press "Safe playbook", then "Use this playbook".
   - See: "Template: Safe playbook", one page ("The route: to the safest beacon, then one priority
     raise per beacon at risk (none by default)"); after Use, "What the playbook says" lists the
     playbook as English. This is the playbook the gateway files for you if a Lull runs out with
     nothing sealed: the wizard's Safe playbook template is the same `safe::safe_playbook` the
     gateway files (item 123 (2) 6), and the editor can render it, which is section 1.1's "the cost
     of a timeout is visible". The client never asks the gateway for it directly.
   - Proof: `crates/gateway/tests/templates.rs`
     `the_gateways_fallback_is_the_safe_template_with_nothing_raised`;
     `crates/gateway/tests/methods.rs` `a_seat_that_seals_nothing_has_the_safe_playbook_filed_for_it`;
     that the page shows it: by hand only.

6. **Fill Hold & Build.** Press "Hold & Build".
   - See: "Template: Hold & Build" and four pages: "Where to walk to before placing: the site
     itself", "Where the Build beacon goes: inside one of your spheres, with the heat vent inside its
     own", "Where the Generator goes: a heat vent inside the new beacon's sphere" and "How long to
     hold by the new beacon, in game milliseconds", each with Easy's suggestion and its why. The
     shape is place-and-build and the values without the wizard are stand-ins; the owner confirmed
     both (item 119 (9) and (10)).
   - Do: keep Easy's suggested places, above all the Generator's anchor: a typed anchor outside the
     new beacon's sphere passes QUICK and FULL and builds nothing (item 113 (15)). Change one value:
     in the hold page, type `20000` and press "Send this value". The page then shows `20000` without
     the mark.
   - Proof: `wizard.rs` `an_edited_value_is_sent_exactly_as_typed_and_alone`; the watch check's
     `_template`.

7. **Press "Use this playbook".**
   - See: "The template's playbook is in the editor. Undo takes it back." Under the buttons the
     verdict goes from "Checking..." to "Quick check - Ready to seal.", and about 600 ms after the
     last edit to "Full check - Ready to seal." Under "Checks": "Nothing to fix." QUICK runs on every
     edit and its rows are icons with plain sentences, but a template the wizard filled qualifies,
     so no row shows here; the Fix button is round 2's (item 123 (2) 5).
   - Proof: `wizard.rs` `using_the_wizard_puts_the_gateways_text_in_byte_for_byte_and_undo_takes_it_back`;
     `crates/client-gdext/src/editor.rs` `an_edit_is_patched_then_quick_then_priced_then_full_after_idle`.

8. **Look at the route.**
   - See: a polyline on the map from the commander through the route's steps, each leg labelled with
     its travel time as raw game milliseconds ("N ms"), and under "Route": "Travel, as estimated: N
     ms". No leg is dashed: terrain is served whole and every leg is priced as known ground (item
     107 (1)).
   - Proof: `editor.rs` `an_edit_is_patched_then_quick_then_priced_then_full_after_idle` (the route
     is priced) and `a_leg_that_names_no_end_draws_no_polyline`; the watch check's `_edit` (the route
     is priced and reachable). That the line is drawn: by hand only.

9. **Optional: one map action.** Click the ground inside your sphere.
   - See: a menu, "Go here" and "Place beacon", and a ghost at the click with "Checking this
     spot...", then "A beacon can go here." or "Not here: ..." The ghost answers per click, not live
     on hover (S3-02). Press "Undo" afterwards to keep Hold & Build as it was.
   - Proof: the watch check's `_edit` (a legal ghost, Place beacon, Undo gives the bytes back);
     `editor.rs` `a_checked_placement_is_taken_without_a_second_call`.

10. **Read the rule list.**
    - See: "What the playbook says": one line for each rule, step or block, in English.
    - Proof: `wizard.rs` `the_rule_list_is_render_plans_lines`.

11. **Press "Submit".**
    - See: "Submitted. This is now your sealed order." and "Checked at submission - Ready to seal."
      Submit runs FULL, and `render_plan`'s English stays in the rule list. The `report_hash` at
      submit equals the FULL pre-check's, which no client view shows (item 123 (2) 6).
    - Proof: `methods.rs` `the_specs_fourteen_call_walkthrough_runs_end_to_end` and
      `crates/gateway/tests/websocket.rs` `the_fourteen_call_walkthrough_also_runs_over_the_transport`
      (the `report_hash` equality, in process and over the wire);
      `tests/golden/gateway/walkthrough/expected.walkthrough.txt` ("accepted=true; report_hash
      matches call 9").

12. **Save the playbook.** Press "Save" and choose a file, say `round1.jsonc`.
    - See: "Saved to <path>." The file is the text on screen, byte for byte.
    - Proof: the watch check's `_edit` and `_template` (a saved file equals the editor's text).

13. **Press "Ready" last.** Ready is final for the rest of the Lull (S6-03), and Easy is always
    ready, so your Ready ends the Lull at once. Do everything above first.
    - See: "all ready" on the status line, perhaps only for a moment, then "Round 1 - PUSH  speed
      1x".
    - Proof: `crates/client-gdext/src/rig.rs` `a_lull_reports_the_clock_and_ends_when_every_seat_is_ready`
      and `ready_waits_behind_the_submission_it_is_ready_with`; `crates/gateway/tests/control.rs`
      `all_ready_reaches_the_admin_and_nothing_else_about_a_seat_does`; the watch check.

## Round 1's Push: nobody in control

The Push runs three minutes at 1× with nobody in control. The speed buttons are "1x", "2x" and "4x"
(the pacer's `SPEEDS`; D-24), and "Skip" plays to the segment's end at once, free, with "(skipping)"
on the status line. Watch round 1 at 1× or 2× so each beat is seen.

14. **Watch with the camera.** Use the free-look keys, the wheel and the right mouse button as in
    step 3. "Follow" toggles following the commander; it is a button, and there is no F key
    **(with T22a**, whose header comment names the button). Typing in a field no longer moves the
    camera, and the keys come back once you leave the field with a right-click on the 3D view
    **(with T22a**, pull request #64).
    - Proof: `crates/client-gdext/tests/pacer.rs` `speed_changes_only_how_much_game_time_the_pacer_asks_for`;
      `control.rs` `skipping_to_segment_end_yields_the_same_terminal_hash_as_playing_it_out`; T22a's
      camera-focus check **(with T22a)**. The camera itself: by hand only.

15. **Read the event list.** Rows appear under the buttons as "m:ss  kind  text", for example "0:00
    push_started  The Push began. This segment runs 3:00." and "plan_sealed  The playbook of seat 0
    was sealed: ..."
    - Proof: `crates/client-gdext/tests/event_list.rs`
      `the_event_list_shows_exactly_what_get_segment_feed_said`; `tests/golden/vista/expected.events.txt`.

16. **The commander walks, places a beacon and interfaces on site.**
    - See: `step_started` and `step_completed` rows as the commander walks its route; "beacon_placed
      A beacon of seat 0 was deployed." (at tick 691, 0:34, in the demo scenario on this seed); the
      visit's rows committing the Build mandate and its Generator target. The treasury does not move
      for the beacon: a placed beacon costs no `$` at the skeleton (found by T22b; X-12).
    - Proof: `scenarios/skeleton/against-easy-three-rounds.scenario.jsonc` asserts seat 0's
      `beacon_placed` by tick 850.

17. **A Generator goes up on the vent and supply rises.**
    - See: "unit_fabricated", "structure_queued  seat 0 paid for a structure; it is going up now."
      (the treasury drops by the Generator's $ 80), then "structure_completed  A structure of seat 0
      is finished." (tick 1276, 1:03, in the scenario). The meter's supply rises from 10 kW by the
      Generator's output, 20 kW on the lean starting vent, and its draw by 1 kW for each unit
      fabricated.
    - Proof: the demo scenario asserts seat 0's `structure_completed` by tick 1550;
      `crates/sim/tests/economy.rs` `only_one_generator_per_vent_adds_to_the_supply`. The meter's
      numbers: by hand only (the watch check's `_meter` checks the meter shows the gateway's four
      numbers in a Push, not what they are).

18. **The Quartermaster's hold is not expected here.** The stub holds a fabricator order when draw
    outruns supply, and a placed beacon adds no draw, so the default match probably never shows a
    hold (item 123 (2) 7).
    - Proof: T22a's Quartermaster hold test, `crates/sim/tests/economy.rs`
      `a_fabricator_order_the_headroom_cannot_run_is_held_until_a_generator_brings_supply_back`
      **(with T22a)**.

19. **Your own ore reaches the treasury.**
    - See: "ore_delivered  A mining drone of seat 0 delivered ore worth $ N." and the treasury rising
      by it. In the scenario seat 0 first delivers at tick 2157 (1:47), after Easy.
    - Proof: by hand only. The demo scenario asserts the first delivery by any seat, which is Easy's,
      seat 1 (item 118 (1)).

20. **Mining remeshes the voxels it edits.** Follow the mining drone: the ground it digs changes
    shape as it goes. Mining is the only thing in the demo that edits voxels; no method can ask for a
    voxel edit. The per-frame budget (K = 4 surfaces, B = 512 KiB) is not visible.
    - Proof: `crates/gateway/tests/view.rs` `an_edit_a_seat_can_see_arrives_with_its_current_bytes`;
      `crates/mesher/src/upload.rs` `the_byte_budget_binds_after_the_first_chunk`. On screen: by hand
      only.

21. **The recap.** At the segment's end:
    - See: "Round 1 - RECAP", and the rows "recap_opened  The recap opened: the Ledger settles." and
      "settled  The Ledger settled and credited seat 0 $ N." BMI is settled by standing: $ 100, less
      5 % for the leader and plus 10 % for last place (the economy golden shows $ 95, $ 102 and $ 110
      for three seats). The recap's
      own settlement lines stay a stub until S1, so the event list's `settled` row is the check (w6
      notes A5).
    - Do: while the recap waits, do step 22. Then press "Continue": every recap waits for it.
    - Proof: the demo scenario asserts `settled` by tick 3600; `economy.rs`
      `the_settlement_ledger_matches_its_golden`; the watch check (Continue leads into round 2's
      Lull).

22. **Prepare round 2's Fix, off the clock.** Copy `round1.jsonc` to `round1-fix.jsonc` and open the
    copy in the text editor. In `"handlers"`, the first rule's last line reads `"resume":
    "CONTINUE", "cooldown_ms": 30000, "max_fires": 3}`: that is `/declarative/handlers/0/cooldown_ms`.
    Change `30000` to `1000`, the value of the verifier's golden, and save. Change nothing else. The
    file carries no fingerprint (the editor writes none), so the edit raises no stale-fingerprint
    E0008.
    - Proof: by hand only. The same edit to `scenarios/skeleton/against-easy.playbook.jsonc`, Hold &
      Build as Easy fills it for seat 0 on this seed, makes `gamectl verify` add E0108 and nothing
      else.

23. **Optional: `gamectl verify` on the saved playbook.** In the zip's folder, run `gamectl verify
    <path to round1.jsonc>`.
    - See: "qualifies    no (1 error)", "error   E0403 /declarative/route/1/place_beacon/at/voxel",
      "the site (x, y, z) lies outside every one of this seat's spheres.", and the footer "Verified
      against the reference seat view ...", although the game accepted the file. `gamectl verify`
      checks against a reference seat view, not your match (items 114 (5) and 117 (1)); the zip's
      `README.txt` tells a tester so.
    - Proof: the watch check's `_template` (a saved file that places a beacon gets E0403 at the
      placed sites and nothing else); `crates/gamectl/src/seat.rs` module doc.

## Round 2's Lull: the carried draft, the notes box, the Fix button

24. **Continue opens round 2's Lull.**
    - See: "Round 2 - LULL  3:00  speed 1x". The editor opens last round's playbook by itself, checked
      again against the new map: "Last round's orders are loaded again and checked against the new
      map (carried from round 1)." and "Quick check - Ready to seal.", then, about 600 ms later,
      "Full check - Ready to seal." (opening the carried draft restarts the idle timer). "Drafts"
      lists "carried from round 1 (round 2)". Easy, from round 2 on, seals its safe playbook at every seed and still says
      ready (item 113 (7)); you cannot see its seat.
    - Proof: `methods.rs` `a_lull_opens_with_last_rounds_playbook_carried_and_re_verified`;
      `editor.rs` `the_carried_draft_is_last_rounds_sealed_playbook_checked_again`; the watch check's
      `_carried`. Easy's later rounds: the demo scenario's hash chain alone.

25. **Write a line in the notes box.** Click into the box under "Notebook" ("Your private notebook.
    Only you see it.") and type a line. The camera does not move while you type **(with T22a)**.
    Press "Save notes".
    - See: "Notebook saved (N characters)."
    - Then: the buttons take no keyboard focus, so the notes box keeps it after "Save notes", and
      the free-look keys stay off until you leave the field **(with T22a)**. Leave it now, before
      round 2's Push (step 30): right-click on the 3D view (a wheel notch there, or a left-click on
      the sky, works too). A left-click on the ground or on a beacon opens the map menu and leaves
      the notes box focused.
    - Proof: the watch check's `_edit` (`save_notes`); `methods.rs`
      `a_second_seats_token_sees_neither_playbook_nor_draft_nor_notebook_nor_replay`.

26. **Load the edited file.** Press "Load..." and choose `round1-fix.jsonc`.
    - See: "Opened." It replaces the carried draft on screen and clears Undo. "Checks" holds one
      row: the word "Error", the sentence "A rule waits a while between firings, so put at least the
      minimum here.", "E0108 at /declarative/handlers/0/cooldown_ms" and the button "Fix: Set the
      cooldown to 5000 ms". The verdict reads "Quick check - Cannot be sealed yet.", then, about
      600 ms later (Load restarts the idle timer), "Full check - Cannot be sealed yet." The row shows
      the verifier's plain sentence for a beginner; the golden's message, "`cooldown_ms` is 1000 ms;
      the minimum is 5000 ms.", is not drawn.
    - Proof: `tests/golden/verifier/e0108_cooldown_below_minimum/expected.report.json` (the code, the
      pointer, the plain sentence and the Fix's title); the watch check's `_edit` loads
      `godot/fixtures/needs_a_fix.jsonc`, the verifier's own E0108 case, and presses its Fix, in the
      `client extension (<os>)` jobs. The hand edit of a saved file: by hand only.

27. **Press "Fix: Set the cooldown to 5000 ms".**
    - See: the verifier's own patch applied through the gateway (the cooldown becomes 5000, not the
      template's 30000); "Nothing to fix."; "Quick check - Ready to seal.", then "Full check - Ready
      to seal." E0109 has no Fix button and is not used here.
    - Proof: `editor.rs` `a_fix_button_is_the_verifiers_own_machine_applicable_patch`; the watch
      check's `_edit`.

28. **Press "Save draft".**
    - See: "Draft saved."; "Drafts" lists "Saved from the editor (round 2)" and "carried from round 1
      (round 2)". Round 3 finds this draft after a resume.
    - Proof: the watch check's `_edit` (`save_draft`).

29. **Press "Submit", then "Ready".**
    - See: as steps 11 and 13, then "Round 2 - PUSH".
    - Proof: as steps 11 and 13.

## Round 2's Push

30. **Watch or skip.** The Push runs five minutes at 1×; "4x" or "Skip" shortens it. If the camera
    keys do nothing, the notes box still has focus: leave it as step 25 says **(with T22a)**. At the
    recap, read the `settled` row and press "Continue".
    - Proof: as steps 14 to 21.

## Round 3's Lull: save and resume

31. **Continue opens round 3's Lull.**
    - See: "Round 3 - LULL  3:00  speed 1x", the carried draft "carried from round 2".
    - Proof: `methods.rs` `a_lull_opens_with_last_rounds_playbook_carried_and_re_verified`; the watch
      check's `_carried`.

32. **Quit in the Lull.** Close the game's window. No button saves the match: the host saves at every
    Lull boundary by itself, and quitting in a Lull closes its standard input, so it saves the Lull
    and exits (item 111, decision C8).
    - See: the window closes, and no `gamectl` process is left (Task Manager, or `ps` on Linux).
    - Proof: `host_loop.rs` `the_host_exits_when_its_control_pipe_closes`; the watch check's resume
      leg (it quits in round 2's Lull and checks that the host exited).

33. **Relaunch and press "Resume last match".**
    - See: the chooser now offers "Resume last match"; after it, "Starting the match host...", then
      "Round 3 - LULL  3:00  speed 1x": a resumed Lull restarts its timer in full, because the timer
      is the client's (H-05, owner at hardening). The notebook holds your round 2 line, "Drafts"
      lists "Saved from the editor (round 2)" and the carried draft, and the carried draft is open.
    - Proof: `crates/gateway/tests/save.rs` `a_resumed_lull_has_the_seats_notebooks_drafts_and_seals`
      and `a_save_at_every_lull_boundary_of_a_three_round_match_resumes_to_the_same_chain`; the watch
      check's `_run_resumed` (a fresh client resumes through `host_link.gd`). The lobby's Remember and
      Resume: `godot_project.rs` `the_lobby_remembers_one_line_and_no_token` and
      `the_lobby_names_and_resumes_only_through_host_links_helpers`, which pin the source; a person
      sees the lobby's paths run here first (item 114 (5)).

34. **Submit and press "Ready".** Submit the carried draft as it is, then Ready.
    - **Optional, a real timeout:** press nothing instead, and let the timer run to 0:00. The Lull
      ends without your Ready, and the gateway files the safe playbook for seat 0, because it sealed
      nothing this round: the Push shows the commander keeping to the safest beacon instead of the
      carried route. This gives up round 3's own orders.
    - Proof: as steps 11 and 13; for the timeout, `methods.rs`
      `a_seat_that_seals_nothing_has_the_safe_playbook_filed_for_it`.

## Round 3's Push: kill the client, resume, and the same Push replays

35. **Kill the client mid-Push.** Once a few rows are in the event list, end the game's process:
    Task Manager's "End task" on `Pharmakos.exe`, or `kill` on Linux.
    - See: the window disappears, and the host exits with it: no `gamectl` process is left, on either
      operating system (T16a notes).
    - Proof: `host_loop.rs` `the_host_exits_when_its_control_pipe_closes` (a closed pipe; a killed
      client closes it too). The kill itself: by hand only.

36. **Relaunch and press "Resume last match".**
    - See: the match comes back in round 3's Push, not in a Lull: "Round 3 - PUSH". The save made
      when the Push began is replayed with the same seals, so the event list repeats the same rows at
      the same times, and the Push ends as it would have. Nothing can be re-planned.
    - Proof: `save.rs` `a_sealed_save_resumes_into_its_push_and_offers_no_lull` and
      `a_save_at_every_lull_boundary_of_a_three_round_match_resumes_to_the_same_chain`. On screen: by
      hand only.

37. **The last recap.** Read the `settled` row and press "Continue". The last recap waits for it too.
    - Proof: `crates/sim/tests/runner.rs` `the_round_limit_ends_the_match_when_the_last_recap_closes`
      (the match ends when the last recap closes, not before); pressing it: by hand only.

38. **The match ends on the round limit.**
    - See: "Round 3 - ENDED  speed Nx  The last match has ended, so there is nothing to resume.",
      and "match_ended  The match ended." The fog lifts on the server by itself: Easy's beacons, units
      and edits appear. Press "Whole map" to frame the camera on all of it. The lobby has forgotten
      the match, so the next launch offers no Resume. The skeleton ends on the round limit, because
      with no combat before S2 the one-tick rule cannot end a two-seat match (item 123 (2) 2). For
      the same reason no demo reaches the fog's other unlock, an elimination (the T16a notes' two-seat
      match that ends by elimination); `crates/gateway/tests/security.rs`
      `elimination_lifts_fog_without_reissuing_a_token` alone proves it until S2.
    - Proof: `crates/sim/tests/runner.rs` `the_round_limit_ends_the_match_when_the_last_recap_closes`;
      `view.rs` `the_full_map_unlock_comes_from_a_server_side_policy_change` and its client half,
      `crates/client-gdext/tests/unlock.rs` of the same name; `godot_project.rs`
      `the_lobby_forgets_an_ended_match_and_shows_a_refusal_as_the_host_wrote_it`. No scenario
      reaches the end (the scenario runner keeps the default of six): by hand only in a hosted match.

## Optional: a refused Resume

39. **Refuse a Resume on purpose.** Start a "New match", and close the window in its first Lull.
    In the private match cache, rename that match's `save.json` (in the newest `local-*` folder) to
    `save.json.bak`. Relaunch and press "Resume last match".
    - See: "The match host would not resume the last match: " followed by the host's own words (a
      missing save is refused as "there is no saved match `<id>` to resume"). The chooser stays
      hidden, so "New match" cannot be reached without restarting the game, and the next launch
      offers the same Resume again (S6-22, owner at
      S6). Rename the file back to resume that match, or delete `last_match.txt` to forget it.
    - Proof: `crates/gateway/src/cache.rs` `a_folder_with_a_save_is_reopened_and_never_opened_new`
      (a match with no save is refused, not opened new); `godot_project.rs`
      `the_lobby_forgets_an_ended_match_and_shows_a_refusal_as_the_host_wrote_it` (source pins: the
      lobby shows the host's words). `host_loop.rs`
      `a_resume_line_that_disagrees_with_the_save_is_refused` is another refusal the lobby shows the
      same way. By hand only in the lobby.

## Deviations from section 1.1

Section 1.1 of `docs/design/skeleton-plan.md` is the demo the owner reviews. Item 123's pull request
amended three of its sentences; the demo still differs from it in the eight ways below (item 123 (2)
1 to 8). At the review the owner rules on the round-limit default (D-20), `match.lull_ms` (D-01) and
the other "At this demo" rows of the register, on these deviations, on the plan's PROPOSED mark (item
87), on the deferral of P2's baseline to S5 and P6's to S3's playtest (item 123 (2) 11), and on the
delegation of items 85, 86, 116 (2) and 120, which the skeleton's end returns to the owner (item 123
(3)).

1. **No dashed legs.** Section 1.1 said fogged legs are dashed and labelled as a bound, never as an
   ETA. It now says no leg is dashed at the skeleton (amended by item 123 (2) 1). The demo draws every
   leg with a travel time, because terrain is served whole and the estimator prices every leg as known
   ground; dashed legs come with S3's one fog mask (item 107 (1); S3-05). Nothing to rule on.
2. **The fortified core has high HP and no autocannon, and the match ends on the round limit.** Section
   1.1 now says "high HP; its autocannon arrives with combat at S2" and that the skeleton's demo
   reaches the round limit (amended by item 123 (2) 2). The core has 3 000 HP (`beacon.core_hp`); no
   autocannon is placed, and with no combat the one-tick rule cannot end a two-seat match. Nothing to
   rule on.
3. **Three rounds from the lobby, six by default.** Section 1.1 now says the lobby's round limit is
   three from T22a (amended by item 123 (2) 3). The lobby hosts three rounds **(with T22a)**; the
   sim's default, which a host gets when it names none and which `gamectl scenario run` keeps, stays
   the spec's six. The owner rules on that default (D-20); the settings screen and the Probation
   preset are the owner's at S1 (S1-40).
4. **Every Lull is three minutes.** Section 1.1 puts all its Lull beats in the first Lull. The spec's
   default is five minutes, with ten for the first, and Probation's Lulls are untimed; the rules'
   `match.lull_ms` gives every Lull 180 seconds, and the config line has no Lull field. The demo
   spreads the beats over the three Lulls, says each time that Ready ends the Lull at once, and treats
   a timeout that files the safe playbook as section 1.1's "cost of a timeout" (item 123 (2) 4). The
   owner rules on `lull_ms` (D-01); a change moves every chain and goes to S1 with the Probation
   preset.
5. **The Fix button is reached by hand.** Section 1.1 has QUICK's rows appear "with Fix buttons" on
   every edit. The stage has two machine-applicable fixes, E0108's and E0008's, and neither is
   reachable from a wizard parameter or a map click; a template the wizard fills has no rows at all.
   The demo saves the playbook, edits its cooldown into E0108 by hand and loads it (steps 22, 26 and
   27; item 123 (2) 5). No suggestion is added.
6. **The `report_hash` and the safe playbook are shown by tests.** Section 1.1 has the `report_hash`
   at submit equal the FULL pre-check's and the safe playbook "which the editor can render". No client
   view shows a `report_hash`, and the client never calls `get_safe_plan`. The tests in steps 5 and 11
   prove both, and the wizard's Safe playbook template, the same `safe::safe_playbook`, shows the
   playbook (item 123 (2) 6).
7. **The Quartermaster's hold is not seen.** Section 1.1 has the stub hold fabricator orders when draw
   outruns supply. A placed beacon adds no draw, so the default match probably never shows a hold;
   T22a's sim test proves it (step 18; item 123 (2) 7).
8. **The camera and typing, and the F key.** Free-look read the keys whatever had focus, so typing a
   note moved the camera, and `camera_rig.gd`'s header promised an F key that does not exist. T22a
   makes the camera ignore the keys while a field has focus and corrects the comment to name the
   Follow button; no key is added (steps 14 and 25; item 123 (2) 8) **(with T22a)**.

Smaller differences, each already decided elsewhere:

- Travel times are shown as raw game milliseconds, not the rounded or whole-second figure items 57
  and 61 ask for (S3-18, owner at S3).
- The placement ghost answers per click; the live-on-hover ghost moved to S3 (w6 notes A6; S3-02).
- The speeds are 1×, 2× and 4×: section 1.1's "2–4×" does not say whether 3× is one (D-24).
- "The host can save": saving is automatic at every Lull boundary, and the demo saves by quitting in
  a Lull (item 111, decision C8; S6-07 for a save browser).
- The recap's settlement lines stay a stub until S1; the event list's `settled` row carries the
  credit (w6 notes A5 and A6).
- A resumed Lull restarts its timer in full (H-05), and a refused Resume leaves New match unreachable
  until a restart (S6-22) (item 114 (5)).
- Easy seals its safe playbook after round one and still says ready (item 113 (7)).

Found by T22b, not decided:

- **A placed beacon costs no `$`.** Nothing charges `structures.beacon.cost_dollars` when a beacon is
  deployed (`crates/sim/src/world.rs` `place_beacon`, whose PLACEHOLDER still names T14; X-12), so the
  treasury moves for the Generator and not for the beacon. Section 1.1 does not say it moves, and the
  spec prices a beacon at $ 60.
- **The E0108 row shows the beginner sentence.** The editor draws a diagnostic's plain sentence for a
  beginner where the verifier gives one (`crates/client-gdext/src/editor.rs` `rows_of`), so the row
  reads "A rule waits a while between firings, so put at least the minimum here.", not the golden's
  message (step 26).
- **The notes box keeps keyboard focus after "Save notes".** Every panel button but a diagnostic
  row's Fix is built with `focus_mode = Control.FOCUS_NONE` (`godot/scripts/editor.gd`, `lobby.gd`,
  `wizard.gd`; `rows.gd`'s Fix is freed by the redraw after a Fix), so pressing one does not take
  focus from the notes box, and with T22a's change the free-look keys stay off until the player
  leaves the field. Steps 25 and 30 say so. The way back is a mouse press the GUI does not take: a
  right-click or a wheel notch on the 3D view, or a left-click on the sky. A left-click on the
  ground or on a beacon is taken by `editor.gd` for the map menu first and keeps the focus; that is
  a carry-forward for the next `editor.gd` lane (pull request #64; item 123 (2) 8).

**Ruled at the review (2026-09-29, decisions-log item 126 (2)).** The deviations above are
accepted, the smaller differences with them. Of the three findings, the free beacon (X-12) is fixed
in S1; the E0108 row's beginner sentence is intended; and the notes box keeping focus is acceptable
with the right-click way back. The delegation named in this section's first paragraph is item
124 (4)'s list, with 116 (9), and it is renewed for S1. What the review saw is in
[`skeleton-demo-record.md`](skeleton-demo-record.md).

## For developers

Not demo beats. Each local run of the watch check (`godot/scripts/watch_check.gd`) leaves a
`watch-check-<unix seconds>-<pid>` folder in the private match cache, which nothing prunes until S6's
save browser; delete them by hand when they pile up (item 114 (5); `godot/README.md`).
