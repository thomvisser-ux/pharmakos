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

**You stand in for the first-time tester** (decisions-log item 153 (4)). You are not new to this
game, so a pass is weaker evidence than the gate asks. The fun gate's testers are the first true
first-time users, and the record says so.

## Before you start

The main session does these, and fills them in on the record below:

1. Builds the zip with `cargo xtask package` from `main` after the lane merges, and names its path
   and SHA-256.
2. Checks that the task card below still qualifies on that build, headless, without you watching.
3. Renames `%APPDATA%\Godot\app_userdata\Pharmakos\` so that `user://` starts fresh, and renames it
   back afterwards. Nothing is deleted.

You need a stopwatch. The main session cannot read the screen's timing to the second, so your watch
is the measure. **Don't read "The task card" until the scratch clock starts.**

## The template timing: at most 2 minutes

1. Run `Pharmakos\Pharmakos.exe` and start a match against Easy from the lobby.
2. **Start the watch when round 1's Lull opens.**
3. Pick any template from the list and answer the wizard's pages. Press "Use this playbook".
4. Press "Submit".
5. **Stop the watch when you see "Submitted. This is now your sealed order."** If the verifier
   refuses it, keep going. The refusal counts against the time, and you say so for the record.

Then close the match. Nothing else in it is timed.

## The scratch timing: at most 5 minutes

Do this in a new match, not the next round, so that it starts with a full Lull and with nothing
carried over.

1. Start a new match against Easy.
2. **Start the watch when round 1's Lull opens, and read the card now.**
3. The editor has no "New" command (see "Deviations"), so open the **Safe playbook** template and
   press "Use this playbook". This is your empty playbook. Its one walk doesn't count as one of
   your ten.
4. Add the card's ten steps, then press "Submit".
5. **Stop the watch when you see "Submitted. This is now your sealed order."**

## How long a Lull lasts

Round 1's Lull lasts 10 minutes (`first_lull_ms`), twice the scratch limit, so its countdown won't
end a run that is inside the limit. **Don't press "Ready" before "Submit"**: Ready with nothing
submitted files the safe playbook for you, and that run didn't finish. Tell the main session the
time on the watch, and the run is repeated once in a new match, the record showing both.

## The task card

Use the map's click menu. The places are yours to choose. Each numbered line is one route step.

1. Walk to a spot of ground near your core.
2. Visit your core at **High** priority.
3. Place a beacon on ground inside your sphere.
4. Walk to a spot of ground beside that beacon.
5. Alt-click a vent and place a beacon covering the nearest vent you do not cover.
6. Walk to your core.
7. Visit your core at **Normal** priority.
8. Walk to another spot of ground near your core.
9. Place a beacon on another spot of ground inside your sphere.
10. Walk to your core.

A step that later fails in the Push doesn't count against you: P6 times writing a playbook that
qualifies, not winning with it. The card was checked headless on `sim3a`'s build at `a37fa23`: this shape, the
template's walk included, sealed on the golden seed, placed its beacons and failed no step. The
main session checks it again on the sitting's own build.

## What is recorded

For each timing, at each sitting:

- the time on the watch;
- each refusal you saw, in its own words;
- each mis-click you name;
- your words on typing feel: whether the re-check and the redraw keep up.

After each scratch run the main session reads `seats/0/sealed/1.jsonc` in the match's cache
(`%LOCALAPPDATA%\Pharmakos\matches\<match-id>\`) and counts the route's steps.

## If a timing misses

P6a is the record, so a miss there moves these levers into `ed3b`'s brief before it starts, on your
word (decision 17): chips that open on the value a template suggests, map picks in place of typed
coordinates, and fewer pages in the guided pick. A miss at P6b goes into `prob` or a short added
lane, after cut 1. The editor's model is already largely in Rust, so the spec's first lever is
spent.

## Deviations from section 5.3

- **"From an empty playbook" starts from the Safe playbook template.** The editor has no way to start
  an empty one: a map action needs text to act on, and it answers "Open a playbook first." otherwise.
  The template gives the two blocks the verifier requires, `on_death` and `fallback`, which the map
  menu cannot add, plus one walk. The cost is one wizard page inside the timing, and the tester
  writes all ten steps. The main session decided this and the owner can veto it. Whether a tester
  should get a real "New" command is for P6a's result and `ed3b`'s brief.
- **"Ten route steps" means ten added,** so the sealed route has eleven with the template's walk.
- **P6b's seed.** Section 5.3 says the golden seed. `gwp` moves the lobby's seed to Probation's own
  (decision 13) before P6b, so P6b runs on the seed the lobby then launches. The card names no place,
  so it reads the same on either.

## The record

| Sitting | Build | Template | Scratch | Refusals | Mis-clicks | Typing feel |
|---|---|---|---|---|---|---|
| P6a | | | | | | |
| P6b | | | | | | |
