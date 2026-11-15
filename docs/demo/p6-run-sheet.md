<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# P6 run sheet: the owner's editor timing

Spec section 16's P6 gate, verbatim: "A first-time tester builds a qualifying playbook from a
template in ≤2 min, and writes a 10-step playbook from scratch in ≤5 min. Editor typing feel
(debounced re-check, redraw) is judged here, not by P1". The fallback is "Move the editor model into
Rust; simplify". The protocol is `docs/design/s3-plan.md` section 5.3.

**There are two sittings** (decision 15, decisions-log item 154). **P6a**, after `ed3a` merges, is the
record: the wizard and the map route with the size meter. **P6b**, after `ed3b` merges, confirms that
the rule list did not make either timing slower.

**You stand in for the first-time tester** (decisions-log item 153 (4), risk R5). You are not new to
this game, so a pass is weaker evidence than the gate asks. The fun gate's testers are the first true
first-time users, and the record says so.

**What a pass here shows.** Task card A is built from what the map menu can do today. It places on
ground because no seam can be picked from the map. It has no guard, handler, chip or mandate, because
`ed3b` brings those. So a pass at P6a shows that ten map-menu steps fit in the limit, not that a
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
the measure. Don't open `docs/demo/p6-task-card.md` until the scratch watch starts.

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
   `docs/demo/p6-task-card.md` now.**
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
  the timings, untimed.

After each scratch run the main session reads `seats\0\sealed\1.jsonc` in the match's cache
(`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`) and checks the route against the card: the template's
two steps, then ten steps of the card's kinds in its order, with priority high at 2 and normal at 7,
and step 5 a description or, if you said so, a vent's name. A run that differs doesn't meet the gate's
scratch clause, whatever its time.

As a cross-check on the watch, the main session reads the last `submit_plan` entry's tick in the audit
log. That clock isn't calibrated for this, so a disagreement of more than a few seconds is reported to
you rather than resolved.

## P6b's additions

The main session writes these after `ed3b` merges, because they use its controls:

- **Card A's kinds in a different order,** because you will know the card by then. The record states
  that practice bias beside the result.
- **Card B, timed once after both timings:** one guard, one chip edit and one mandate row. It is
  recorded beside the others and doesn't decide the gate. It measures what card A can't.
- **Untimed, afterwards:** change three number chips and drag one rule. Your words here close the
  typing clause.

## If a timing misses

A miss is a time over the limit. A P6b time over P6a's but inside the limit isn't a miss: it is
recorded and shown to you with the practice and seed caveats.

P6a is the record, so a miss there moves these levers into `ed3b`'s brief before it starts, on your
word (decision 17): chips that open on the value a template suggests, map picks in place of typed
coordinates, and fewer pages in the guided pick. A miss at P6b goes into `prob` or a short added lane,
after cut 1. The editor's model is already largely in Rust, so the spec's first lever is spent. Card A
touches few of these levers, so a scratch miss may point at the map menu and the camera instead, and
the record says which.

## Deviations from section 5.3

- **"From an empty playbook" starts from the Safe playbook template, the nearest thing to empty.** The
  editor can't start an empty one: a map action needs text to act on, and it answers "Open a playbook
  first." otherwise. The template gives you `on_death`, `fallback`, a `flee` handler and two route
  steps, none of which you write, and it costs one wizard page inside the time. So this times writing
  ten route steps, not a whole playbook, and the sheet has told you the way in. The main session
  decided this and the owner can veto it.
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
person's, and the card was read beforehand, not opened when the watch started. The AI's checks
are not independent review.

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
- Mis-clicks: none in the timed runs. The camera opened with the core at the panel's right edge; it
  took 2 s of D and four wheel ticks before the first click, inside the scratch time. The first
  ground click, about 20 voxels from the core, drew "Not here: A beacon goes up inside ground you
  already hold." beside the Go here / Place beacon menu; a spot nearer the core drew "A beacon can go
  here."

**The void match.** One earlier match, `local-1794786057-2556`, is void: the AI's tool launched extra
copies of the game before any template was used. It says nothing about the editor, and no timing was
repeated.

**The hand checks.**

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
first-time person does, so the gate's two timing clauses are not shown for a person and stay open
for P6b, which the owner drives by hand. The findings carried into `ed3b`'s brief:

1. Alt-click on a vent never gives the description.
2. The re-check and redraw are not noticeable to a watcher: nothing draws the eye to the status line
   or the size meter after an edit.
3. The map opens too far out to read, and the core sits at the panel's edge: open framed on the
   seat's core at a zoom where beacons and vents read.
4. There is no "New" command (the Safe playbook template stands in).
5. The "Not here" sentence shows in the side panel, away from the click.
6. The E0601 checkbox and the structure names still need a hand check, so P6b's sheet needs a set-up
   that shows them (a committed playbook that builds an autocannon, a wall and a mortar, and one that
   goes short of power).

## The record

One row per sitting and timing. Fixed for every row: the tester is the owner, not a first-time user
(item 153 (4), R5); card A is shaped to the map menu; and the AI's checks are not independent review.

| Sitting | Timing | Build, SHA-256 | Seed, preset | Template, values changed | Time | Limit, pass or miss | Repeat? | Refusals | Mis-clicks | Route count, card match | Alt-click check | Edit or typing feel |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| P6a | Template | zip from `83c63a0`, `bb05bd62741ddaa78c8022b817537bd1adc32588a1477359480aef22cd4e8a98` | golden `0x00000000ca5caded`, round 1's Lull 10 min | Expand & Mine, none | about 0:15 (countdown 9:45; audit tick 246, 12.3 s) | 2 min: inside the limit, AI-driven | none (`local-1794786057-2556` void, see above) | none | none | n/a | n/a | no edit made; see the scratch row |
| P6a | Scratch | zip from `83c63a0`, `bb05bd62741ddaa78c8022b817537bd1adc32588a1477359480aef22cd4e8a98` | golden `0x00000000ca5caded`, round 1's Lull 10 min | Safe playbook, none | about 1:52 (countdown 8:08; audit tick 2183, 109 s) | 5 min: inside the limit, AI-driven | none | none | none | 12, matches; step 5 a vent's name | failed: no description menu, for the AI and for the owner by hand | "i did not see the re-check and redraw. most of the map has been too far zoomed out for me to see useful detail." |
| P6b | Template | | | | | 2 min | | | | n/a | n/a | |
| P6b | Scratch | | | Safe playbook | | 5 min | | | | | n/a | |
| P6b | Card B | | | | | none | | | | | n/a | |
