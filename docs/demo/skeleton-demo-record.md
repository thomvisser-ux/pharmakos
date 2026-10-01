<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Stage demo record: the walking skeleton (2026-09-29)

What the owner's review of the walking skeleton's demo saw, step by step against
[`skeleton-run-sheet.md`](skeleton-run-sheet.md), what it found, and what the owner ruled.
Decisions-log §2.7 item 126 records the close; this page is its evidence. Times are the demo
machine's local time (EDT, UTC−4).

## The setup

- **Build:** the draft prerelease `v0.1.0-dev.2`'s Windows zip,
  `pharmakos-v0.1.0-dev.2-windows-x86_64.zip`, SHA-256
  `755bb8c142728c6832c52d77a7fe99780952d30db0bf8cd9b391f2f085f9cf7b`, matching the release's
  `SHA256SUMS.txt`. Extracted to a folder of its own, outside the repository; `gamectl seat doctor`
  inside it: six checks, all well.
- **Machine:** Windows 10 Pro 19045, i7-9800X, 32 GB, Quadro P4000 (Vulkan, Forward+).
- **Who drove:** the owner started it; after the first match the main session drove the game by
  computer use, one instance at a time, with the owner watching and reporting what only a person
  sees (a firewall prompt, a console window, the machine's load).
- **Match:** the run sheet's match was `local-1790722223-9868` (two seats, the owner and Easy, three
  rounds from the lobby), which left three `replay/<round>.hashes.txt` files in its match cache.

## Before you start: the four things only a person sees

1. **Rendering:** the terrain renders under the ash-grey sky with the map framed whole; units and
   beacons are small dots at whole-map zoom.
2. **GPU:** starts and renders on the Quadro P4000 (Vulkan).
3. **Firewall prompt at New match:** none.
4. **A console window for `gamectl` at New match:** none. `gamectl.exe` ran as a child of
   `Pharmakos.exe` with no window, and closing the game ended it shortly after (item 108: the child
   exits when its pipe closes).

## A first match, before the run sheet

The owner's first New match ran out its Lull while the owner read. The timeout filed the safe
playbook, and the Push played it: `plan_sealed` "The playbook of seat 0 was sealed: 1 route steps.",
`push_started` 3:00, step 0 started and completed, `fallback_engaged` "posture code 2"; the economy
read $ 200, supply 10 kW, draw 4 kW, headroom 6 kW, as the run sheet says. A relaunch's "Resume last
match" resumed it (the audit log's `resumed` at tick 5 038) and ran on to tick 6 022. A second
`Pharmakos.exe` started at 18:47:40, while or just after the first ran, hung "Not Responding" with
an empty log and no `gamectl` child, and was ended by hand (finding F3). The run sheet's match was
then started fresh with New match.

## The steps

Steps 4 and 5 (the templates' tour and the Safe playbook's page) were skipped for time, and the
optional steps 9 (a map action) and 39 (a refused Resume) were not run. Every other step was run;
what differed or went unverified is in the table and the findings. Step 20's remeshing was not
noted.

| Step | Seen |
|---|---|
| 1–3 | Launch, New match, and a look at the map in round 1's Lull. |
| 6 | Hold & Build: four pages with Easy's values, "suggested by the built-in operator" and a Why paragraph; the hold typed from 81 200 to 20 000, and the Send-this-value mark cleared. |
| 7 | "The template's playbook is in the editor. Undo takes it back.", then "Full check - Ready to seal."; Checks: "Nothing to fix." |
| 8 | A short route line at the core. In round 2's Lull the Route panel read "Travel, as estimated: 29600 ms" and the Budget "7 of 128 size units" and "14.5 s standing at a beacon along the route"; the per-leg "N ms" label was not seen. |
| 10 | The rule list in English: "Hold & Build", "Filed by the built-in operator.", "Grow the grid: …", Route 1–3, Rules (F5). |
| 11 | "Submitted. This is now your sealed order." / "Checked at submission - Ready to seal." |
| 12 | Save with no file yet did nothing visible (F6); Save as… opened the native dialog and saved `round1.jsonc`. |
| 13 | Ready with 0:57 left on the Lull's clock (the brief "all ready" was not seen): "Round 1 - PUSH"; `plan_sealed` "3 route steps"; `push_started` 3:00. |
| 14–21 | Follow tracks the commander; the wheel and Q/E barely moved the camera under synthetic input, so the camera by hand is unverified. 0:22 step 0 done; 0:34 `beacon_placed`; 0:36 `row_committed` "Interface row 0 committed", step 1 done, `structure_queued` "seat 0 paid for a structure; it is going up now"; 0:37 `unit_fabricated`; 0:56 step 2 done and `fallback_engaged`; 1:03 `structure_completed`. After the placing: $ 100, supply 10, draw 5, headroom 5 (the beacon cost nothing: X-12, seen live); after the Generator: supply 30 kW, headroom 25 kW. Ore: 1:47 $ 64, 2:41 $ 32. 3:00 `segment_ended` after 3 600 ticks, the recap, `settled` "credited seat 0 $ 95". Treasury $ 291 = 100 + 64 + 32 + 95. 2× and 4× work. The owner: performance good, no lag or load. |
| 18 | The Quartermaster's hold: not seen, as the run sheet expects. |
| 22 | `round1-fix.jsonc` = `round1.jsonc` with `handlers/0` `cooldown_ms` 30 000 → 1 000, a one-line diff. |
| 23 | `gamectl verify round1.jsonc`: FULL, qualifies no, one error, E0403 at `/declarative/route/1/place_beacon/at/voxel` "the site (337, 20, 36) lies outside every one of this seat's spheres.", with the footer naming the reference seat view, as the run sheet says. |
| 24 | Continue: "Round 2 - LULL", the carried draft loaded ("Last round's orders are loaded again and checked against the new map (carried from round 1).", "Full check - Ready to seal."), Drafts "carried from round 1 (round 2)". The status line still read "speed 4x" (F6). |
| 25 | A note typed and saved ("Notebook saved (46 characters)."); W then typed into the box and the camera stayed; a right-click on the 3D view released the focus and W moved the camera, as the run sheet says. |
| 26 | Load… `round1-fix.jsonc`: "Opened."; Checks: Error, "A rule waits a while between firings, so put at least the minimum here.", E0108 at `/declarative/handlers/0/cooldown_ms`, "Fix: Set the cooldown to 5000 ms"; "Full check - Cannot be sealed yet." |
| 27 | Fix: "Nothing to fix.", "Full check - Ready to seal.", Undo enabled; the first click on Fix did nothing and the second applied it (F1). |
| 28 | Save draft: Drafts lists "Saved from the editor (round 2)". |
| 29 | Submit (again on the second click, F1), Ready: "Round 2 - PUSH", `plan_sealed` "3 route steps". |
| 30 | Round 2's Push at 4×: the same beats at the same times, `beacon_placed` again at the same site (F4); 5:00 `segment_ended` after 6 000 ticks, `settled` $ 110; treasury $ 401. No `ore_delivered` (F2). |
| 31 | Continue: "Round 3 - LULL", carried from round 2. |
| 32 | The window closed in the Lull: `Pharmakos.exe` and `gamectl.exe` both gone, the audit log ends "save lull ok", `save.json` written. |
| 33 | Relaunch: "Resume last match" (the chooser also shows the speed buttons and the editor panel, F6); Resume: "Round 3 - LULL 2:53 speed 1x" with the timer restarted in full (H-05), the carried draft, $ 401, the notebook's line from round 2, and both drafts. The event list starts empty after a resume. |
| 34 | Submit (on the second click after a panel scroll, F1, the third time), Ready: "Round 3 - PUSH", "This segment runs 8:00." |
| 35 | At 2×, rows to 0:57, then `taskkill /F` on `Pharmakos.exe`: `gamectl.exe` exited with it; `save.json` had been rewritten at the Push's start. |
| 36 | Relaunch and Resume: "Round 3 - PUSH" replays from 0:00 with identical rows at identical times (0:22, 0:34, 0:36, 0:37, 0:57). No Lull offered. |
| 37 | Round 3's Push, resumed, at 4×: 8:00 `segment_ended` after 9 600 ticks, `settled` $ 95, treasury $ 496 = 401 + 95. No `ore_delivered` (F2). Continue on the last recap. |
| 38 | "Round 3 - ENDED", "The last match has ended, so there is nothing to resume.", `match_ended` "The match ended."; the panel reads "No templates."; the fog lifted by itself, and Whole map framed Easy's units (and a red marker) at the bottom corner. The lobby forgot the match: `user://last_match.txt` is gone. |

## Findings

Into S1's first lane, F1 first (item 126 (2) (h)):

- **F1. The first click after the editor panel scrolls or changes is swallowed.** Seen on Fix once
  and on Submit twice; the second click works each time. Reproducible. Cause not yet known (a click
  landing while a redraw frees the row, or focus handling).
- **F2. No `ore_delivered` in rounds 2 and 3.** Round 1 delivered $ 64 and $ 32; rounds 2 and 3,
  with the same carried playbook, delivered nothing. Not yet explained.
- **F3. A second instance hangs.** A second `Pharmakos.exe` started while one runs hung "Not
  Responding" with an empty log and no `gamectl` child. The candidate cause is two instances sharing
  `user://` or the shader cache; it is to reproduce first.
- **F4. The carried Hold & Build re-places the free beacon at the same site every round.** Each
  round's Push placed a beacon again at the site of round 1's, free each time. It shows X-12 (a
  placed beacon is never charged), which the owner rules is fixed in S1; whether a carried route
  should place again at a site it already holds is for S1's first lane.

With them, smaller:

- **F5. Wording.** A timeout's filing reads like the player's own seal (nothing in the event list
  says the safe playbook was filed); "1 route steps"; `fallback_engaged` shows "posture code 2", a
  raw code; "Filed by the built-in operator" shows on a playbook the player chose from a template
  and edited.
- **F6. Minor.** Save with no file does nothing visible (only Save as… works); the Push's speed
  label carries into the Lull, where speed means nothing; the chooser shows the speed buttons and
  the whole editor panel before any match; the first launch's log holds two expected `ERROR` lines from negative probes,
  the client check's rather than, as first written, the shipped smoke check's (S1's `fixc` traced
  them; item 129), which read as errors in a player's log. The
  camera's wheel zoom was not verified, because synthetic input barely moved it; the owner checks it
  by hand.

## Rulings

The owner, 2026-09-29, on the main session's recommendations ("go with your recommendations"):

- The walking skeleton is accepted and the stage is closed (AGENTS.md section 10 item 8).
- The register's "At this demo" rows D-01 to D-27 stand as they are for now, among them the 180 s
  Lull (D-01), the sim's default of six rounds (D-20) with the lobby's three, and the camera's and
  the pacer's values (D-21 to D-24).
- Skeleton-plan's PROPOSED mark (item 87) is lifted.
- The run sheet's deviations from section 1.1 are accepted, all eight (the summary put to the owner
  counted seven), on the recommendation and open to the owner's overrule.
- P2's baseline goes to S5 and P6's to S3's playtest (item 123 (2) 11).
- The run sheet's three undecided findings: X-12, the free beacon, is fixed in S1; the notes box
  keeping focus is acceptable with the right-click way back; the E0108 row's beginner sentence is
  intended.
- The delegation of items 85, 86, 116 (2) and (9), and 120 is renewed for S1.
- F1 to F4 go into S1's first lane, F1 first; F5 and F6 go with them.
