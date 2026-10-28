<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# The nightly adversarial arms

Rusher, Turtle and Hunter: three hand-written playbooks played against the
built-in operator, Easy, on a fixed seed set every night, each with a written
alarm band (AGENTS.md §10 item 5; spec §15; S2's plan, task `night`, and
decisions 18, 29 and 30; decisions-log item 144 (6)).

```text
scenarios/nightly/
  README.md                           this file
  seeds.txt                           the fixed seed set, twenty seeds
  rusher.scenario.jsonc               the three arms the workflow plays
  rusher.playbook.jsonc                 and the playbook each seals
  turtle.scenario.jsonc
  turtle.playbook.jsonc
  hunter.scenario.jsonc
  hunter.playbook.jsonc
  placeholder.playbook.jsonc          what the result fixture seals
  fixture/result-golden.scenario.jsonc  the arm the result golden is played from
  fixture/two.seeds.txt                 and its two seeds
```

## The three arms

Each arm is one hand-written playbook in the v1 vocabulary (spec section 15;
S2's plan, task `arms`), and one file plays **every round of every seed, from
seat 0 on half the seeds and seat 1 on the other half**, and must qualify every
time: a refusal fails the night. That shapes all three. A playbook can name
nothing one seat or one map owns, so places are descriptions read when their
step starts ("the nearest scrap seam", "the nearest uncovered vent", "the safest
beacon"), never a beacon by name, and the route, which every round replays
from the top, guards each placement with a `skip_if` (a beacon of that mandate
already stands, or the grid already has the kW to spare), so a later round
adds no second post. Each sets `allow_dormant_beacons`, because the verifier's
power projection does not read those guards.

| Arm | What it does | Where it departs from spec section 15, and why |
| --- | --- | --- |
| **Rusher** | A Survey beacon first, whose two scouts visit the eight columns from which a scout sees any core the generator can place (each spawn point with every coordinate of 24 or 359 moved 16 voxels towards the centre, inside a scout's 24-voxel sight of a core there); then an Attack beacon, at HIGH priority beside the core; it launches on two raiders (`force_size: 2`), `FREE_FIRE`, no retreat, at the nearest known enemy beacon (`nearest` with the filter `side: ENEMY_KNOWN`): the nearest enemy beacon the seat has seen inside `verifier.reach_memory_ms` and does not see gone, nearest the staging point at the map's centre, ties to the lowest id, chosen at launch; then `NEAREST_KNOWN`, the next nearest enemy beacon the seat knows of; a generator only while the grid has under 4 kW to spare, which on the twenty seeds it never is. | In its opening and in what comes after the first target. The archetype is an early Attack beacon; this arm places a Survey beacon first, because a raid on the nearest known enemy beacon waits until the seat knows one, and its raid launches in round 2, not round 1: round 1 goes on the two posts and the raiders' walk to the centre (aimed at a fixed voxel, it launches in round 1 on two seeds only). The archetype then goes for the core; `NEAREST_KNOWN` takes the next nearest enemy beacon the seat knows of, which is the core only when the core is the nearest known one, because no Attack target names an enemy's core in v1. Its first target is the archetype's since S2's `atk2` (decisions-log item 150 (2)): with no sighting at the seal the verifier qualifies the target with `W0709`, and the sim holds the gathered force, counted, until the seat knows an enemy beacon. The staging point is the centre (192, 192), the one voxel that means the same from every spawn, so "nearest" is measured from there rather than from the post. Since S2's `rush`, measured over the twenty seeds, the raid launches in round 2 on every seed (ticks 4 768 to 5 792; round 2 opens at tick 3 600), where without scouts it launched on none. That is measured, not checked: a night checks no assertion, and the per-pull-request test plays round 1 only. |
| **Turtle** | A Defend beacon beside the core guarding the core and "my commander" (`FREE_FIRE`, the whole sphere, one picket); the core and every Build beacon repair below 75 % and rebuild, which fields repair drones; a generator on the nearest free vent while the grid has under 6 kW to spare, otherwise an autocannon `on` the nearest free vent; no Attack beacon and no raider of its own, so the treasury hoards what the guns and repairs leave. | The walls. A `wall` target is a box of fixed voxels, one map's and one seat's, so no wall goes up. The guns stand at the nearest free vents, not at the core, whose only gun is its own built-in autocannon. |
| **Hunter** | A Survey beacon with two scouts over the whole map, placed first; an Attack beacon launching three raiders, which the seat can afford from round 2, at `enemy_commander` with no seat named: the nearest enemy commander the seat has seen inside `verifier.reach_memory_ms`, nearest the staging point at the map's centre, ties to the lowest seat id, chosen at launch; then `NEAREST_KNOWN`. | None in its target since S2's `vfy4`: with no sighting at the seal the verifier qualifies the target with `W0708` rather than `E0418` (the owner's ruling of 2026-10-27), and the sim holds the gathered force, counted, until a fresh sighting exists. The staging point is the centre, the one voxel that means the same from every spawn, rather than a place the archetype names. |

One `PLACEHOLDER` in the playbooks is for the owner: the Turtle's walls
(decisions-log item 150 (3)). While it stands, the Turtle's band still reads
as decision 18 writes it, but it measures the arm as built, a guarded core
without walls, not the archetype spec section 15 names, so a low rate there is
a floor and not a balance reading. The Hunter's band, since S2's `vfy4`, reads
its archetype: a hunt for the enemy commander; the Rusher's, since S2's
`rush`, reads its own: an early raid on the nearest known enemy beacon. On the run the `arms` lane attached
(its pull request), every match of the sixty ended on the round limit, so every
result was the final audit's; none ended by elimination.

Literals in the playbooks come from the rules table's map, 384 x 384 x 64: the
centre (192, 192), the Hunter's probe box (0, 0, 0) to (383, 383, 63), and the
Rusher's eight probe columns at 40, 192 and 343 on each axis, which are the
generator's eight spawn points (`map.spawn_zone_radius_voxels` 24 in from each
edge, at the corners and the edges' midpoints) with each coordinate of 24 or
359 moved 16 voxels towards the centre. A core stands at most 4 voxels off its
spawn point along each axis, inward only at a corner (the generator's jitter,
clamped to the band), so at most about 22.6 voxels from its column. A change
to the map's size or the spawn radius moves what they mean, and a change to
the scouts' `vision_radius_voxels` row below 23 (the distance from a corner's
column to its core) leaves the scouts short of a core. A literal that a change
puts off the map is refused, which `crates/gamectl/tests/nightly.rs` reports
on the pull request that makes it. The `kw_headroom` thresholds (4 and 6 kW)
are behaviour, not refusals: a change to the power rows (S2's `tune2`) moves
which step runs, and `demo2`'s forced run on those numbers is where that
shows.

`NIGHTLY_SCENARIOS_ENABLED`, the repository variable the workflow is gated on,
stays unset until `arms` has merged and its forced run is attached (decision
30); only the owner sets it.

## An arm is a scenario file with two rules of its own

The format gains no key for the nightlies (S2's plan, E20). An arm is an
ordinary `*.scenario.jsonc` file (`scenarios/README.md`), and both readers,
`xtask/src/scenario.rs` and `crates/gamectl/src/scenario.rs`, read two more
rules from where it lives:

* **No `hash_chain_equals`.** A run over a seed file replaces `map.seed`, so a
  chain for the file's own seed would pin a match the nightly never plays. The
  arm still asserts on events, which are checked when it is played on its own
  seed.
* **Two seats: one `playbook` seat, the arm, and one `builtin` seat, Easy.**

`cargo xtask ci`'s `scenario` step validates every arm and plays none of them,
and `cargo xtask package` never replays one: both take `collect_played`, which
leaves this directory out. What runs on every pull request instead is
`crates/gamectl/tests/nightly.rs`: each arm's playbook must qualify, and each
arm plays its first round on its own seed through `submit_plan`, from seat 0
and from seat 1, with no refusal.

## A night

```sh
gamectl scenario run scenarios/nightly/rusher.scenario.jsonc --operator easy \
  --seeds scenarios/nightly/seeds.txt --format json --out rusher.json
```

plays the arm once per seed, **to the match's end**: the file's segments are
the match's segment lengths, rounds past the last listed segment take its
length, and the match ends where the sim ends it (the last seat standing, no
survivor, or the round limit). The file's assertions are not checked. The arm
alternates seats by the seed's position in the file: even positions (counting
from 0) put the arm on seat 0, odd ones on seat 1 (decision 29). Every round
the arm's playbook goes through `submit_plan`; a refusal fails the run, naming
the seed and the round, and the harness door is never taken.

`--operator` names the operator the `builtin` seat plays: `easy`, which
carries the Balanced weighting, is the only value this build has, and the
bands are re-baselined against Normal at S5 (item 144 (6)).

## The result and the band

Per seed: the seed, the arm's seat, the winners, the end reason, the end tick,
the rounds and the arm's outcome. The arm **wins** a seed only as the sole
winner; a **shared** win is not a win for the band (decision 29). The summary's
`win_rate_percent` is `arm_wins x 100 / seeds`, an integer rounded down (M-01).
`tests/golden/scenarios/nightly-result/expected.result.json` pins the JSON's
shape.

### The bands

Written per S2's plan, decisions 18 and 29 (ruled by decisions-log item
144 (11)), one per arm:

* **Rusher:** an arm that is the sole winner — last seat standing, or the
  final audit's — in more than 13 of the 20 seeds (65 %) against Easy opens a
  tuning task. It watches spec section 19's "an early rush ends matches before
  authoring matters" and the core's HP and turret strength. From S2's `atk2`
  the arm's target is the nearest known enemy beacon itself, and from S2's
  `rush` its scouts give it one: measured over the twenty seeds, the raid
  launches in round 2 on every seed, so this band reads an early raid on the
  nearest known enemy beacon, launched in round 2, not a floor. The result
  does not say whether or when a seed's raid launched, and nothing checks
  that it still does: the measurement was `attack_launched` asserted per
  round on each seed, by hand. A change to the scouts' vision, the spawn
  layout or Survey that stops the launch would make this band a floor again
  without a red check. Measured at `rush`, the arm was the sole winner of 8 of the 20
  seeds (40 %), every match ending on the round limit, against 0 of 20 with no
  scouts.
* **Turtle:** an arm that is the sole winner — last seat standing, or the
  final audit's — in more than 13 of the 20 seeds (65 %) against Easy opens a
  tuning task. It watches "hoarding or turtling wins, now that value follows
  condition".
* **Hunter:** an arm that is the sole winner — last seat standing, or the
  final audit's — in more than 13 of the 20 seeds (65 %) against Easy opens a
  tuning task. It watches "commander hunting dominates", whose counters
  (armour, decoys, a self-defence weapon) are added only if this band shows it
  (spec section 4). From S2's `vfy4` the arm hunts the enemy commander itself,
  so this band is a reading of the archetype, not a floor a departure sets.
  A seed on which no round gives the seat a sighting never launches the hunt,
  and the result does not say which seeds those were.

At S2 every band reads against Easy, which carries the Balanced weighting
(item 144 (6)); each is re-baselined against Normal at S5. A shared win counts
as no win, which favours not alarming (decision 18's downside). 14 sole wins
of 20 is 70 % and alarms; 13 is 65 % and does not. Each band is stated over
the committed `seeds.txt`: a manual run on another seed file (the workflow's
`seeds` input) is not a band reading. Each is an alarm, not a
gate: `.github/workflows/nightly-scenarios.yml`'s band step warns and fails
nothing, and a breach is a tuning task, never a tuning change in the pull
request that saw it. S2's close acknowledges the three bands as Easy's (item
144 (6)), with the departures above stated beside them.

## The seed set

`seeds.txt`: one seed per line, `0x` and exactly sixteen lowercase hex digits
(a scenario's own `map.seed` spelling), `#` comments, blank lines skipped. The
decoder caps the file at 64 KiB, a line at 256 bytes and the set at 1 024
seeds, and refuses a malformed line or a repeated seed with its line number
(`crates/gamectl/src/scenario/seeds.rs`, whose property test is
`crates/gamectl/tests/seeds.rs`). The twenty are the S1 balance check's
(`crates/gamectl/tests/balance.rs`), so the nightly and the balance report
play the same maps; they are committed and never regenerated, and
`tests/seeds.rs` holds the two lists equal. Its first ten seeds are also G4's bar: the
workflow's `g4` job plays ten seeded matches of 9 600 ticks on each of the
three operating systems and byte-compares their chains. A seed changed here
is a new night, and the pull request that changes it says so.
