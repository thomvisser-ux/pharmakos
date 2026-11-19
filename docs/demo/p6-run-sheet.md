<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# P6 run sheet: the owner's editor timing

Spec section 16's P6 gate, verbatim: "A first-time tester builds a qualifying playbook from a
template in ≤2 min, and writes a 10-step playbook from scratch in ≤5 min. Editor typing feel
(debounced re-check, redraw) is judged here, not by P1". The fallback is "Move the editor model into
Rust; simplify". The protocol is `docs/design/s3-plan.md` section 5.3.

**There are two sittings** (decision 15, decisions-log item 154). As planned, **P6a**, after `ed3a`
merged, was the record: the wizard and the map route with the size meter; and **P6b**, after `ed3b`
merges, was to confirm that the rule list did not make either timing slower. P6a was then driven by
the AI (see "P6a as run"), and decisions-log item 160 (4) records the main session's recommendation,
which you can veto, that P6b, which you drive by hand, decides instead.

**You stand in for the first-time tester** (decisions-log item 153 (4), risk R5). You are not new to
this game, so a pass is weaker evidence than the gate asks. The fun gate's testers are the first true
first-time users, and the record says so.

**What a pass here shows.** Task card A was built from what the map menu could do at P6a. It placed
on ground because no seam could be picked from the map. It had no guard, handler, chip or mandate,
because `ed3b` had not yet brought those. So a pass at P6a shows that ten map-menu steps fit in the limit, not that a
tester can write the playbook they want. That weakens the evidence beyond R5's caveat, and the record
says so too.

## Before you start

The main session does these, and fills them in on the record:

1. Builds the zip with `cargo xtask package` from `main` after the lane merges, and names its path
   and SHA-256.
2. Checks card A headless on that build, starting from the text the wizard answers on that build.
3. Renames `%APPDATA%\Godot\app_userdata\Pharmakos\` so that `user://` starts fresh. Afterwards it
   renames the sitting's folder to `Pharmakos.p6a` (or `.p6b`) and puts the original back. Nothing is
   deleted.

You need a stopwatch. The main session can't read the screen's timing to the second, so your watch is
the measure. Don't open `docs/demo/p6-task-card.md` until the scratch watch starts (at P6b, each of
its two cards until its own watch starts).

Each timing is its own match. Closing the window ends the host: there is no close control. The
opponent is fixed as Easy.

## The template timing: at most 2 minutes

1. Run `Pharmakos\Pharmakos.exe` and press "New match".
2. **Start the watch when the status line first shows round 1's Lull.**
3. Pick any template except Safe playbook. Keep the suggested values, or change one and press "Send
   this value" before "Use this playbook". A value you type and don't send isn't used.
4. Press "Use this playbook", then "Submit".
5. **Stop the watch when you see "Submitted. This is now your sealed order."** If the verifier
   refuses it, keep going: the refusal counts against the time, and you say so for the record.
6. Close the window.

## The scratch timing: at most 5 minutes

1. Run `Pharmakos\Pharmakos.exe` and press "New match", never "Resume last match".
2. **Start the watch when the status line first shows round 1's Lull, and open
   `docs/demo/p6-task-card.md` now** (at P6b, `docs/demo/p6b-task-card-a2.md`).
3. Open the **Safe playbook** template and press "Use this playbook". This is your starting playbook.
   Its two steps (the walk to the safest beacon and the core's dig depth) don't count as your ten.
4. Add the card's ten steps, then press "Submit".
5. **Stop the watch when you see "Submitted. This is now your sealed order."**
6. Untimed: press "Ready — end the Lull" and wait for the Push to begin, which writes the sealed file.
   Then close the window.

## If a run goes wrong

**Don't press "Ready" before "Submit".** Ready with nothing submitted files the safe playbook for you.
If you do, the run is recorded as unfinished with its time, and repeated once in a new match. The
repeat's time stands, marked as card already seen. This is the only repeat: a run over the limit isn't
repeated, and neither is a run the Lull ended. Both are misses.

At P6a, round 1's Lull lasts 10 minutes (`first_lull_ms`), twice the scratch limit. At P6b the lobby's
Probation preset has untimed Lulls, so no countdown shows.

A step that later fails in the Push doesn't count against you: P6 times writing a playbook that
qualifies, not winning with it.

## What is recorded

For each timing, at each sitting:

- the time on the watch, and the template you picked with any value you changed;
- each refusal you saw, in its own words;
- each mis-click you name;
- your words on the re-check and the redraw after each edit. **At P6a nothing typed feeds the re-check,
  so this is edit feel, not typing feel, and the gate's typing clause stays open until P6b;**
- at P6a, five hand checks, each a failure fixed in `ed3b`: whether Alt-click on a vent showed the
  description menu (the check `s3-plan.md` owes for item 139 (2) C); and, from `ed3a`, whether Escape
  closes a menu on a real window, whether Load's dialog opens with an empty box, whether the
  unticked checkbox reads as unticked, and whether you can read each structure's name and tell an
  autocannon, a wall and a mortar apart, close up and from about 50 voxels (names draw within 55). The last four come after
  the timings, untimed;
- at P6b, the same for card B and the untimed part: each refusal and each mis-click you name; your
  words on the re-check and the redraw after each edit and on typing into the number fields, and
  whether each number went in whole; the three hand checks (Alt-click on a vent, the E0601
  checkbox, the names); and your answers to the two questions under the record.

After each scratch run the main session reads `seats\0\sealed\1.jsonc` in the match's cache
(`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`) and checks the route against the card: the template's
two steps, then ten steps of the card's kinds in its order, with priority high at 2 and normal at 7,
and step 5 a description or, if you said so, a vent's name (at P6b, card A2: high at 1, normal at 9,
and step 2 the description or the name). A run that differs doesn't meet the gate's scratch clause,
whatever its time.

As a cross-check on the watch, the main session reads the last `submit_plan` entry's tick in the audit
log. That clock isn't calibrated for this, so a disagreement of more than a few seconds is reported to
you rather than resolved.

## P6b's additions

Written after `ed3b` merged (#183 and #188), for the build below. Every card and set-up here was
checked on that build; "How P6b's cards were checked" says how.

**You drive P6b by hand, and P6b decides.** Decisions-log item 160 (4) records the main session's
recommendation, which you can veto, that decisions 15 and 17 are read on P6b rather than on the
AI-driven P6a. So a P6b time is read against the limits, not against P6a's.

- **Card A's kinds in a different order** (card A2), because you will know card A by then. The
  record states that practice bias beside the result.
- **Card B, timed once after both timings:** one mandate row, one guard and one chip edit. It is
  recorded beside the others and doesn't decide the gate. It measures what card A can't.
- **Untimed, afterwards:** change three number chips and drag one rule, then the hand checks P6a
  didn't make and the one it failed.
- **P6a's times are no baseline for "not slower".** They were driven by the AI, not by a person (see
  "P6a as run"), so a P6b time is read against the limits, and beside P6a's only with that caveat.
- **The practice bias** stated beside every P6b timing (template, scratch and card B) includes
  having watched card A driven at P6a.
- **The scratch timing starts from the Safe playbook**, as at P6a and as the first deviation below
  says (the main session's recommendation; you have been told).

### Before you start, for P6b

1. **The build** (done): `D:\build\p6b\play\Pharmakos\Pharmakos.exe`, packaged from `main` at
   `39d0d13c` (after `ed3b` part 2, #188), zip `D:\build\p6b\play\pharmakos-windows-x86_64.zip`,
   SHA-256 `c3c5446ff4b0d34342c5ba1ea6c1de68eca43b0421a8f159af1b263a5a746a8e`.
2. **The cards** (done): card A2, card B and the two set-up files, checked on that build.
3. **`user://`** (the main session, just before the sitting): step 3 above, the sitting's folder
   renamed `Pharmakos.p6b` afterwards.
4. **The set-up files** (the main session, before the sitting): copies of
   `docs/demo/p6b-short-of-power.jsonc` and `docs/demo/p6b-structures.jsonc` in
   `D:\build\p6b\play\Pharmakos`, beside `Pharmakos.exe`.

### The Probation facts

Read these before the first watch starts.

- The lobby hosts the Probation preset: seed `0x0000000000000014` (20), two seats, you at seat 0
  and Easy at seat 1, three rounds.
- **The Lulls are untimed.** No countdown shows, and only Ready ends a Lull. Your stopwatch is the
  only clock; the audit log's tick still cross-checks it.
- Your core stands on flat ground, and its sphere reaches 24 voxels. The map opens on your
  commander, at your core.
- **The vent.** The heat vent nearest your core is about 40 voxels from it. On the map's opening
  view it should lie to the left of your core and a little below it on the screen (worked out from
  the map's axes and the camera's opening angle, not seen on a window). A vent is drawn in the
  blue-teal of its ore, against grey stone and the green and brown of the ground; a scrap seam is
  ochre. Clicked, the panel heads its menu "Heat vent (175, 66)". The next vents are about 80
  voxels out.
- At round 1's Lull the meter reads `$200   supply 10 kW   draw 4 kW   headroom 6 kW`.

### The order of the sitting

Four matches, each from "New match", with the window closed between them:

1. The template timing, as above.
2. The scratch timing, as above, with card A2.
3. Card B, timed.
4. Untimed: the chips, the drag and the hand checks.

### Card A2

`docs/demo/p6b-task-card-a2.md`. It differs from card A in three ways and no other: the same ten
kinds of step in another order, no step in card A's place; the vent step takes one of the vent
menu's two rows, where card A asked for an Alt-click; and it is played on the Probation map. It
gives the same level of hint as card A, and the places are still yours. The route check is the one
above. **Practice bias:** you know card A's kinds and its controls, and you watched card A driven at
P6a.

### Card B

1. Run `Pharmakos.exe` and press "New match".
2. **Start the watch when the status line first shows round 1's Lull, and open
   `docs/demo/p6b-task-card-b.md`.**
3. Do the card.
4. **Stop the watch when you see "Submitted. This is now your sealed order."**
5. Untimed: press "Ready — end the Lull", wait for the Push to begin, and close the window.

The main session reads its sealed route: the template's two steps, then a visit to `b_00` that adds
an autocannon, skipped if the grid's headroom is below 3 kW.

### Untimed, afterwards

Launch the fourth match from a Command Prompt (cmd, not PowerShell, which can drop the `--`), in
the game's folder, so that every map click's modifiers go to the instance's log, which the main
session reads afterwards:

```sh
cd /d D:\build\p6b\play\Pharmakos
Pharmakos.exe -- --log-input
```

Press "New match". In round 1's Lull:

1. **Alt-click on a vent** (the check P6a failed). Hold Alt and click the vent (see "The Probation
   facts"). You should see "Place beacon covering the nearest vent you can cover" first and "Place
   beacon covering this vent" second. Press Escape. Click it again without Alt: the same two, the
   other way round. Press Escape. Since `ed3b` both are always offered, and Alt only puts the
   description first. **Write down** which came first each time.
2. **The unticked E0601 checkbox** (check 4). Press "Load...". The dialog may open in any folder:
   paste `D:\build\p6b\play\Pharmakos` into its address bar, press Enter, and open
   `p6b-short-of-power.jsonc`. It adds three mortars at your core; a mortar draws 3 kW and you have
   6 kW of headroom. When the full check has run, about a second after it opened, the rows under
   "Checks" show an Error, "There is not enough power for everything this playbook switches on.",
   marked "E0601 at /declarative/route/0", and under them the checkbox "Allow dormant beacons: let
   this route draw more power than you supply, so some beacons go dark." A Warning row is there too,
   before and after the tick, and is expected: "This playbook spends more than the seat is likely
   to have." (W0602). Tick the box: the status line says "Dormant beacons allowed.", E0601 gives
   way to W0603, and the box is ticked. Untick it, and E0601 is back. **Write down** whether the box
   read as unticked before you ticked it, and as ticked after.
3. **Three number chips and one rule drag.** Press "Load..." and open `p6b-structures.jsonc` from
   the same folder (`D:\build\p6b\play\Pharmakos`). Under "What the playbook says", change three
   chips: 40 in flee's "at or below 40% HP" to 30; 3 in "Fires at most 3 times" to 2; 1 in
   regroup's "has died at or above 1 times" to 2. For each, click the chip, select the old number in
   the "Change ...:" field first (double-click it, or Ctrl+A), type the new one, press "Set this
   value", and read the chip afterwards: it should say the new number. A chip that holds the old and
   new digits together (for 40 to 30, "4030" or "3040") means the typing went in beside the old
   number: that is a finding to write down, not a miss; set it right and go on.
   Then drag rule 2's line ("regroup") onto rule 1's ("flee"): it lands before it. **Write down**
   your words on the re-check and the redraw after each edit, and on typing into the field.
4. **The structure names** (check 5). Press "Submit", then "Ready — end the Lull". In the Push the
   core's drones build an autocannon, a wall and a mortar beside your core. A structure's name
   shows from when it is placed, about 0:10 into the Push, before it stands: at 1x the autocannon
   stands about 0:26, the wall about 1:05 and the mortar about 1:22 (2x and 4x shorten the wait).
   The map opens too far out for any name: a name draws only within 55 voxels of the camera, and
   the map opens at 120. So wheel in until the names show, then out until they go, and note about
   how far that was. **Write down** whether you can read each name and tell the three apart, close
   up and at the farthest you can still read them. Then close the window.

**What "typing" means on this build.** Nothing you type into the playbook feeds the re-check as you
type: a chip's number is sent when you press "Set this value", and the re-check follows the edit.
Whether your words in item 3 close the gate's typing clause is your question to answer, in the
record below.

**What was promised and isn't on this build.**

- **A wall has no name.** A wall is wall voxels in your colour, not a structure the view lists, so
  only the autocannon and the mortar carry a name; the wall is told apart by its look.
- **No rule to drag except from a file.** Dragging a rule onto another is built, but every template
  on this build has one rule and there is no "Add rule" control, so the drag uses
  `p6b-structures.jsonc`, which has two.
- **The two set-ups are files, not clicks.** No control can give a wall its box, so the names
  set-up has to be a file. The E0601 set-up could be clicked (three "Add a build target" mortars),
  but a file is quicker and is the one that was checked.

**Known gaps to step round** (PR #188's owed list): "Queue a structure" offers Generator,
Autocannon, Mortar and Wall, though the verifier refuses every structure there but the Resonance
Spire; "Add a build target" offers Wall, but no control can give a wall its box; a duration (3 s,
30 s) is drawn as the gateway wrote it and can't be edited; and a choice's values keep their quotes
(`"HIGH"`).

### How P6b's cards were checked

On the packaged build itself: its own `gamectl.exe host`, started on the lobby's Probation line
(seed 20, two seats, seat 0 the human's, three rounds) with its data folders in a scratch folder,
and reached on its two connections as the lobby reaches it. An uncommitted script made the calls
the editor's controls make: `instantiate_template` for the Safe playbook's text; one `patch_plan`
per card step, carrying the patch the client composes (the map menu's step, the Visit & change
row, the condition slot's `add` as its `pick_op` names, the chip's `replace`); QUICK after each
edit, FULL at the end, and `render_plan` for the rule list's words; then `submit_plan`, Ready,
`end_lull`, and round 1's Push played to its end with the segment feed read. The places were one
set near the core, at the ground's height read from the view, so it doesn't show that every place
you might click works.

- **Card A2**: QUICK qualified after every edit; FULL qualifies, 12 steps, 15 of 128 size units;
  accepted. In the Push all 12 steps completed and three beacons were placed, the vent's at 0:41.
  From New's empty playbook instead of the Safe playbook, the same ten steps also qualify and
  complete.
- **Card B**: FULL qualifies, 7 size units; accepted. All three steps completed, and the autocannon
  stood at 0:21.
- **`p6b-short-of-power.jsonc`**: QUICK qualifies, so Load opens it; FULL raises E0601, "the
  playbook adds 3 kW of draw beyond supply", with "Allow dormant beacons" as its suggestion, and with
  that applied it qualifies with W0603.
- **`p6b-structures.jsonc`**: qualifies; the autocannon stood at 0:24, the wall filled from 0:50 to
  1:05, and the mortar stood at 1:22. The three chip edits and the rule move each still qualify.
- `gamectl verify` is no check here: it verifies against its fixed reference view, whose core is
  `b_01`, so every `b_00` in these files is E0401 there.

## If a timing misses

A miss is a time over the limit. A P6b time over P6a's but inside the limit isn't a miss: it is
recorded and shown to you with the practice and seed caveats.

As planned, a miss at P6a, the record, was to move these levers into `ed3b`'s brief before it
started, on your word (decision 17): chips that open on the value a template suggests, map picks in
place of typed coordinates, and fewer pages in the guided pick. That did not happen: P6a was driven
by the AI, so decisions-log item 160 (4) reads no miss from it, and `ed3b` (#183 and #188, now
merged) took your P6a findings instead of the levers wholesale. A miss at P6b goes into `prob` or a
short added lane, after cut 1. The editor's model is already largely in Rust, so the spec's first lever is spent. Card A
touches few of these levers, so a scratch miss may point at the map menu and the camera instead, and
the record says which.

## Deviations from section 5.3

- **"From an empty playbook" starts from the Safe playbook template, the nearest thing to empty.** The
  editor can't start an empty one: a map action needs text to act on, and it answers "Open a playbook
  first." otherwise. The template gives you `on_death`, `fallback`, a `flee` handler and two route
  steps, none of which you write, and it costs one wizard page inside the time. So this times writing
  ten route steps, not a whole playbook, and the sheet has told you the way in. The main session
  decided this and the owner can veto it.
  [Since #183 (item 161 (3)) the editor can start an empty playbook: New puts
  `godot/playbooks/new.jsonc` on screen. Which start the timed card uses is not ruled here.]
- **"Ten route steps" means ten added,** so the sealed route has twelve.
- **The card is opened by you when the watch starts,** rather than shown by the main session, and it
  stays readable throughout. Opening it is inside the time.
- **The repeat rule above isn't in section 5.3.** It covers one mistake that says nothing about the
  editor.
- **Card B and the P6b order change aren't in section 5.3.** Neither decides the gate.
- **P6b's seed and preset.** Section 5.3 says the golden seed, which holds at P6a. `gwp` moves the lobby
  to Probation before P6b: its own seed, chosen for a near vent, and untimed Lulls. So P6b's times
  aren't a like-for-like comparison with P6a's.
- **P6a was driven by the main AI session with the owner watching,** at the owner's request, so the
  watch was the Lull's countdown read from the screen, cross-checked against the audit log's ticks.
  The driver knew card A before the watch started, because it had made the card check, so the card
  was not first opened when the watch started. The countdown was read from screenshots, so it runs
  late by the AI's latency; the audit log's tick is the earlier bound. Between matches the AI closed
  the window and launched the game again.
- **P6a restarted a void match, which the repeat rule doesn't cover.** In `local-1794786057-2556`
  the AI's tool launched extra copies of the game after the client had opened a template
  (`instantiate_template` at tick 126 in its audit log) and before any playbook was checked or
  submitted. The match was abandoned and the template timing ran in a new one. The repeat rule
  covers only Ready before Submit, so this restart is outside it.
- **P6b's untimed Lulls.** Section 5.3 times from round 1's Lull opening; at P6b no countdown shows
  and only Ready ends a Lull, so your stopwatch is the only clock.
- **P6b's names check tells the wall by its look.** A wall has no name on this build (see "What was
  promised and isn't on this build").
- **P6b's rule drag and both of its set-ups are loaded from files,** not built with the editor's
  controls: no template has two rules, and no control gives a wall its box.
- **P6b's typing clause.** Nothing typed feeds the re-check as you type on this build, so the
  clause is put to you as a question under the record rather than read from your timings.

**The missing "New" command is a finding in its own right,** whatever P6a's time, and so is the opening
camera, which leaves the core at the panel's edge. The main session carries both into `ed3b`'s brief.

## How the card was checked

A headless probe, not committed and not made through the editor's clicks: the wizard's Safe playbook
text for the golden seed plus card A's ten steps at one set of places, with `safe` in Easy's seat, on
`sim3a`'s build at `a37fa23`. It sealed, placed its beacons and failed no step. That holds only for
the probe's own places: step 5 can read a different vent if step 3's beacon already covers the near
one.

## P6a as run (2026-11-15)

**Who drove.** The owner asked the main AI session to drive the game window by computer control,
and watched. So both timings are AI-driven: they measure the AI's clicks (seconds of latency per
action, no reading time, no hunting for controls), not a person's. The AI had read task card A
beforehand, because it made the headless card check. Another project's cargo job was running; no
Pharmakos build was. That weakens the evidence beyond the caveats above: the clicks were not a
person's, and the card was read beforehand, not opened when the watch started.

**Before you start, as done.**

1. The build is named below.
2. Card A's headless check is the one under "How the card was checked", on `a37fa23`. No check on
   the P6a build (`83c63a0`) is recorded; the scratch match's sealed route, read against the card
   below, is the only reading of the card on that build.
3. `%APPDATA%\Godot\app_userdata\Pharmakos\` was renamed `Pharmakos.orig-p6a` before the sitting,
   and the leftovers of a headless watch check run on 2026-11-12 were set aside as
   `Pharmakos.p6a-check`. Afterwards the sitting's folder was renamed `Pharmakos.p6a` and the
   original put back. Nothing was deleted.

**The build.** `D:/build/p6a/Pharmakos/Pharmakos.exe`, packaged from `main` at `83c63a0` (after
`ed3a`, #166), zip `pharmakos-windows-x86_64.zip`, SHA-256
`bb05bd62741ddaa78c8022b817537bd1adc32588a1477359480aef22cd4e8a98`. The lobby's golden seed
`0x00000000ca5caded`, Easy opponent, round 1's Lull 10:00.

**The timings.** The time is 10:00 less the Lull's countdown when "Submitted. This is now your sealed
order." showed, cross-checked against seat 0's `submit_plan` tick in the audit log at 20 Hz.

- Template: Expand & Mine, no value changed. Submitted at countdown 9:45, about 0:15; the audit log
  has tick 246 (12.3 s). No refusal. Match `local-1794786192-15984`.
- Scratch: Safe playbook, then card A's ten steps. Submitted at countdown 8:08, about 1:52; the audit
  log has tick 2183 (109 s). No refusal. Match `local-1794786235-6836`. The two clocks agree within a
  few seconds.
- The sealed route (`seats/0/sealed/1.jsonc`) has 12 steps: `to_safety`, `deepen_core`, then `go_1`
  (move, voxel), `visit_1` (`b_00` set_priority HIGH), `place_1` (voxel), `go_2` (move, voxel),
  `place_2` (place_beacon covering `feature_id` `vent_324_16`), `go_3` (move to `b_00`'s anchor),
  `visit_2` (set_priority NORMAL), `go_4` (move, voxel), `place_3` (voxel), `go_5` (move to `b_00`'s
  anchor). It matches the card, with step 5 a vent's name, not a description. The Push began and the
  log read "The playbook of seat 0 was sealed: 12 route steps."
- Mis-clicks: none named, because the owner, who would name them, did not drive. The camera opened
  with the core at the panel's right edge; it took 2 s of D and four wheel ticks before the first
  click, inside the scratch time. The first ground click, about 20 voxels from the core, opened the
  Go here / Place beacon menu at the click while the side panel read "Not here: A beacon goes up
  inside ground you already hold."; a spot nearer the core read "A beacon can go here." Step 1 is a
  walk there (`go_1`, a move), so it was not redone. The scratch match's audit log has 13 seat-0
  `patch_plan` calls for the ten steps. A ground click's placement preview is a `patch_plan` call
  too (the editor's `Job::Preview`), and the log doesn't record which call was which, so the three
  extra calls are not accounted for one by one.

**The void match.** One earlier match, `local-1794786057-2556`, is void: the AI's tool launched extra
copies of the game after the client had opened a template (tick 126) and before any playbook was
checked or submitted. It says nothing about the editor, and no timing was repeated; the restart is
under the Deviations above.

**The hand checks,** made untimed after the two timings in match `local-1794786427-18332`. The AI
made checks 2 and 3; check 1 was made by the AI and by the owner by hand; checks 4 and 5 were not
made.

1. Alt-click on a vent: **failed.** The AI's Alt-click and plain click both showed "Place beacon
   covering this vent"; the owner's own Alt-click by hand: "I get Place Beacon". Nobody saw "Place
   beacon covering the nearest vent you can cover".
2. Escape closes a map menu on a real window: yes.
3. Load's dialog opens with an empty File name box: yes (a native Windows dialog, folder
   `D:\build\p6a\Pharmakos`, filter "Playbooks (*.jsonc)").
4. The unticked E0601 checkbox: **not checked.** Four plain place_beacon steps did not make power
   short and the map menu cannot add a structure, so the checkbox never showed.
5. Structure names (autocannon, wall, mortar, close and from about 50 voxels): **not checked.** No
   template builds all three and the map menu cannot.

**The owner's words.** On the re-check and the redraw after each edit: "i did not see the re-check
and redraw. most of the map has been too far zoomed out for me to see useful detail." On the
sitting: "i watched the playthrough. seemed to go well. not much got tested imo".

**What it shows** (the main session's reading; the owner can veto it). Ten map-menu steps fit well
inside the limits when the clicker knows the card and the controls. It does not show that a
first-time person does, so the gate's two timing clauses are not shown for a person and stay open.

The findings, for the main session to weigh when it writes `ed3b`'s brief; they are not `ed3b`'s
settled scope:

1. Alt-click on a vent never gives the description. (Already in `ed3b`'s section of `s3-plan.md`, as
   item 139 (2) C.)
2. The re-check and redraw are not noticeable to a watcher: nothing draws the eye to the status line
   or the size meter after an edit. (Would widen `ed3b`.)
3. The map opens too far out to read, and the core sits at the panel's edge: open framed on the
   seat's core at a zoom where beacons and vents read. (The framing is already in `ed3b`'s section;
   the zoom would widen it.)
4. There is no "New" command (the Safe playbook template stands in). (Already in `ed3b`'s section.)
5. The "Not here" sentence shows in the side panel, away from the click. (Would widen `ed3b`.)
6. The E0601 checkbox and the structure names still need a hand check, so P6b's sheet needs a set-up
   that shows them (a committed playbook that builds an autocannon, a wall and a mortar, and one that
   goes short of power). (For P6b's sheet, not `ed3b`; see "P6b's additions".)

**Open for the owner: decisions 15 and 17.** Decision 15 makes P6a the record, and decision 17
moves the "simplify" levers (see "If a timing misses") into `ed3b`'s brief on a P6a miss. An
AI-driven sitting is neither a pass nor a miss for a person, so neither decision has its input. The
main session recommends that P6b, driven by the owner by hand, become the deciding sitting, and that
`ed3b` take the owner's findings rather than the three levers wholesale. That is a recommendation
awaiting the owner's word, not a ruling; when the owner gives it, it needs a decisions-log item.
Until then the two P6a rows below are marked "not decided". [Since then decisions-log item 160 (4)
records it, still as the main session's recommendation that the owner can veto, and `ed3b` took the
owner's findings. The P6a rows stay "not decided".]

## The record

One row per sitting and timing. Fixed for every row: the tester is the owner, not a first-time user
(item 153 (4), R5), except in P6a's two rows, which the main AI session drove by computer control
with the owner watching; card A (and P6b's card A2) is shaped to the map menu; and the main session's
checks (the card's headless check, the sealed route read against the card, the audit log's
cross-check) are the AI's own, not independent review.

Every P6b row (template, scratch and card B alike) carries the practice bias: you know card A's kinds
and the controls, and you watched card A driven at P6a.

| Sitting | Timing | Build, SHA-256 | Seed, preset | Template, values changed | Time | Limit, pass or miss | Repeat? | Refusals | Mis-clicks | Route count, card match | Alt-click check | Edit or typing feel |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| P6a | Template | zip from `83c63a0`, `bb05bd62741ddaa78c8022b817537bd1adc32588a1477359480aef22cd4e8a98` | golden `0x00000000ca5caded`, no preset (the build predates presets: the lobby's fixed match), round 1's Lull 10 min | Expand & Mine, none | about 0:15 (countdown 9:45; audit tick 246, 12.3 s) | 2 min: inside the limit, driven by the main AI session by computer control with the owner watching, so not a first-time person's time; not decided (see the open question) | none (`local-1794786057-2556` void, see above) | none | none named (the owner did not drive) | n/a | n/a | no edit made; see the scratch row |
| P6a | Scratch | zip from `83c63a0`, `bb05bd62741ddaa78c8022b817537bd1adc32588a1477359480aef22cd4e8a98` | golden `0x00000000ca5caded`, no preset (the build predates presets: the lobby's fixed match), round 1's Lull 10 min | Safe playbook, none | about 1:52 (countdown 8:08; audit tick 2183, 109 s) | 5 min: inside the limit, driven by the main AI session by computer control with the owner watching, so not a first-time person's time; not decided (see the open question) | none | none | none named (the owner did not drive) | 12, matches; step 5 a vent's name | failed: no description menu, for the AI and for the owner by hand | as a watcher: "i did not see the re-check and redraw. most of the map has been too far zoomed out for me to see useful detail." |
| P6b | Template | zip from `39d0d13c`, `c3c5446ff4b0d34342c5ba1ea6c1de68eca43b0421a8f159af1b263a5a746a8e` | Probation `0x0000000000000014` (20), preset `probation`, untimed Lulls | | | 2 min | | | | n/a | n/a | |
| P6b | Scratch | zip from `39d0d13c`, `c3c5446ff4b0d34342c5ba1ea6c1de68eca43b0421a8f159af1b263a5a746a8e` | Probation `0x0000000000000014` (20), preset `probation`, untimed Lulls | Safe playbook, card A2 (practice bias: card A's kinds known, card A watched at P6a) | | 5 min | | | | | n/a | |
| P6b | Card B | zip from `39d0d13c`, `c3c5446ff4b0d34342c5ba1ea6c1de68eca43b0421a8f159af1b263a5a746a8e` | Probation `0x0000000000000014` (20), preset `probation`, untimed Lulls | Safe playbook | | none | | | | | n/a | |
| P6b | Untimed and hand checks | zip from `39d0d13c`, `c3c5446ff4b0d34342c5ba1ea6c1de68eca43b0421a8f159af1b263a5a746a8e` | Probation `0x0000000000000014` (20), preset `probation`, untimed Lulls | `p6b-short-of-power.jsonc`, `p6b-structures.jsonc` | none | none | n/a | | | n/a | | |

P6b's checks 4 and 5, which the table has no column for:

| P6b hand check | Set-up | What you saw |
|---|---|---|
| 4. The E0601 checkbox: unticked before, ticked after | `p6b-short-of-power.jsonc` | |
| 5. The names (autocannon, mortar; the wall by its look), close up and at the farthest you can read them (note about how far) | `p6b-structures.jsonc`, round 1's Push | |

Two questions for you to answer at the sitting:

1. "Nothing you type on this build feeds the re-check as you type; a number goes when you press 'Set
   this value'. Do your words on the number fields close the gate's typing clause, or does it stay
   open?"

   Your answer:

2. "A wall has no name on this build. Is telling it by its look enough for check 5?"

   Your answer:
