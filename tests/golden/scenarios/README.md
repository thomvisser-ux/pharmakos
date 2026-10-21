<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `scenarios/` — what the scenario runner asserts on

**Filled by T11** (the first full-segment hash chain) and T15 (the runner);
T18b added `against-easy` and T20 the demo scenario, `against-easy-three-rounds`:
three rounds against Easy, whose 6 000-line chain pins every tick of all three
(its scenario file's header says what it depends on, including `crates/operator`
and `library/`). S1's `fixs` lane added the first two of `scenarios/s1/`:
`round-limit-audit`, six rounds to the round limit whose final tick carries the
final audit's winner in the match state's winner byte, and
`unaffordable-deploy`, whose fourth deploy fails with `unaffordable` (failure
id 12) because the first three spent the treasury; and it moved three of the
skeleton's chains, because a deploy is now charged its `$` when it starts
(register X-12; the paragraph on S1's `fixs` below says where each first
diverged). Every
chain here is also compared across Windows, Linux and macOS by the
`cross-OS determinism guard` job.

One directory per scenario, named after its `name` key:

```text
tests/golden/scenarios/<name>/expected.hashes.txt   the per-tick chain
tests/golden/scenarios/<name>/expected.events.txt   the event log its producing
                                                    test writes (expand-east-
                                                    segment only)
tests/golden/scenarios/nightly-result/expected.result.json
                                                    the nightly result's shape
                                                    (S2's `night`; below)
```

**S2's `night` added `nightly-result/expected.result.json`**, the one golden
here that is not a chain: the JSON result (`pharmakos.nightly-result.v1`) that
`gamectl scenario run <arm> --seeds <file> --format json` writes, for a
two-seed run of `scenarios/nightly/fixture/result-golden.scenario.jsonc` over
`scenarios/nightly/fixture/two.seeds.txt`. `crates/gamectl/tests/nightly.rs`
produces it and compares it in the same test. Per seed it holds the seed, the
arm's seat (seat 0 for the first seed and seat 1 for the second, S2's plan,
decision 29), the winners, the end reason, the end tick, the rounds and the
arm's outcome (`win` only as the sole winner); then the summary, whose
`win_rate_percent` is `arm_wins x 100 / seeds` rounded down (M-01). One seed
per line, so a diff reads seed by seed. **It moves** when the format moves
(a contract change, AGENTS.md section 5), or when either match ends
differently: the placeholder playbook, Easy (`crates/operator`), the template
library, the rules table (whose hash it carries) or the seeds. The nightly
arms themselves carry no golden (S2's plan, E20); the workflow uploads each
night's result as an artifact instead.

The chain is the half a scenario file names, in its `hash_chain_equals`
assertion, and every scenario has one. **The event log is optional and only
`expand-east-segment` carries one**, because `crates/sim/tests/scenario.rs`
produces it by driving the sim directly and can therefore see a sim event's
`seq`, `subject` and `value`. `gamectl scenario run` reads the gateway's
segment feed instead — the events as a *match* reports them — which carries a
kind, an audience and a time and not those three fields. It writes no event
log, and the `event_fired` assertions inside the scenario file are the event
half of that scenario's claim: they are checked on every run, by the runner,
and a scenario with none of them is refused by the format.

Re-bless a chain with **`cargo xtask golden --bless`**, after
`cargo test --workspace` has produced the fresh one. Every chain here is
written during `cargo test` — `crates/gamectl/tests/scenarios.rs` plays every
committed scenario — so a bless never needs the binary to have been run by
hand.

**`expand-east-segment` has two producers, and this is deliberate.**
`crates/sim/tests/scenario.rs` drives the sim's own runner and writes both the
chain and the event log; `crates/gamectl/tests/scenarios.rs` plays the same
file through a gateway-hosted match and writes the chain. Both write
`actual.hashes.txt` in the same place, and **both compare it against the
committed copy inside their own test** — so if the two ever disagreed, the
producer would say so rather than the `golden` step silently judging whichever
ran last. That the two agree byte for byte is the thing worth having: the sim's
runner and a match hosted behind a `Surface` are the same match.

The chain has the same format as `determinism/`: tick, TAB, sixteen lowercase
hex digits, one line per tick, LF, trailing newline. `scenarios/README.md`
documents the scenario files themselves.

## What a diff means

* **The chain moved.** Read `determinism/README.md` — it is the same news, on a
  seed that a scenario happens to name.
* **The event log moved but the chain did not.** Impossible if the events are
  derived from hashed state, so this is either an event that is *not* hashed
  state (fine, and it should say so) or a bug. Decide which before blessing.
* **The chain moved but the event log did not.** The sim's behaviour changed in
  a way no assertion in this scenario covers. That is worth a moment: the
  scenario may be asserting less than it should.
* **A scenario passed after an assertion was loosened.** The pull request must
  say so out loud. A weakened check reads exactly like a fixed bug in the
  summary line and nowhere else.

## What `expand-east-segment` does *not* cover

Read its `summary` key before reading its chain. The spec's worked playbook does
not get far on this map at these tuning values: the first step walks east for
the whole of its 120 s timeout without arriving, the eastern site is out of
placement range, and the tail names another seat's beacon. So the log holds no
`beacon_placed`, no `visit_started`, no `row_committed` and no `rule_fired` —
the segment pins the interpreter **refusing** loudly, not the interpreter
working. The interpreter working is pinned by `interpreter/`'s five transcripts
and by the determinism chain, whose harness playbook completes a visit and a
deploy. A second scenario whose playbook finishes its route is worth having and
is not this task's to add (one scenario file per lane).

What it *does* cover from T14 onward is the **economy running underneath a
playbook that is going nowhere**: each seat's starting mining drone finds ore in
its core's sphere and delivers it (twice for seat 0 and once for seat 1 since
S1's `mine`, below), and the Ledger settles
at the recap. Those `ore_delivered` and `settled` lines come from the mandate
layer rather than from the playbook, which is the point — a seat that seals
nothing still earns and is still paid.

**T15 added it: `deploy-and-visit`.** Four route steps and all four complete —
a walk, a deploy that puts a beacon in the ground at tick 451, a walk home and
an interface row committed on the core at 776 — with seat 1 sealing nothing so
that the gateway files the safe playbook for it. Read the two together: this
one is what the interpreter does when a playbook fits the map, that one is what
it does when a playbook does not, and neither is the whole picture on its own.

**T18b added `against-easy`**, whose seat 1 is `builtin` and played by the
built-in operator, Easy, and whose seat 0 seals Hold & Build as Easy fills it,
so its chain depends on `crates/operator` and `library/` as well as on the map
seed, the playbooks and the rules hash, and a change to either moves it.

**T14b moved both chains from their first line (tick 1)** and nothing else
here: the key-core rule (decisions-log item 113 (5)) changed every seat's
settled `kW` draw, which is hashed seat state, and changed no event these
scenarios report. `expand-east-segment`'s event log did not move, and every
tick `deploy-and-visit` asserts on or quotes (206, 451, 511, 716, 776) is where
it was. That is the "chain moved but the event log did not" case above, read
and accepted: the scenarios assert on the route, not on the meter.

**S1's `fixs` moved three chains and left the fourth** (register X-12, spec
section 7: "`$` leaves the treasury the moment an order commits: deploy
starts"). A deploy is now charged `structures.beacon.cost_dollars` on the tick
it starts, and the treasury is hashed seat state, so each chain moved on the
first tick a deploy started: `against-easy` and `against-easy-three-rounds`
from tick 206 (Easy's seat 1 starts its Mine deploy), and `deploy-and-visit`
from tick 211 (seat 0's deploy). `expand-east-segment` and its event log did
not move: its deploy fails `illegal_site` as it starts, before the charge, so
nothing is ever paid. The two `scenarios/s1/` chains are new.

**S1's targeting (`tgt`) moved every chain from its first line (tick 1)**
(decisions-log item 127 (12) and (13); `docs/design/targeting.md`). The
canonical encoding gained columns -- each beacon's per-seat ordinal, each Build
target's bound feature and description, each seat's step bindings and restart
point -- so every tick's hash moved whatever the match did. What the matches
*did* moved in two places, both read: `expand-east-segment`'s event log, where
`b_01` is now each seat's own first placed beacon (per-seat names), which
neither places, so seat 0's tail fails `no_target` (2) instead of the retired
`not_own` (10) and seat 1's fails the same way instead of walking to its core
(tick 2411); and `against-easy-three-rounds`, where seat 0's carried deploy is
refused in rounds 2 and 3 by no stacking (`illegal_site`, ticks 4056 and 5191)
instead of placing a second beacon on its round-1 column (the demo's F4).
Every tick the other scenarios quote is where it was. `cover-nearest-vent` is
new.

**S1's Mine mandate (`mine`) moved every chain from its first line (tick 1)**
(register S1-38; decisions-log items 127 (9) and 128 (3) (e)). The encoding
gained five beacon columns -- the four Mine settings and the seam a beacon
holds -- and, measured with them left out, every chain's behaviour also first
diverges at tick 1, because every seat mines: its starting drone now works the
seam its core holds, chosen on the decision tick after tick 1's programs
phase, so it stands for that one tick, and from then on it digs from pit-safe
stands and takes its seam's whole top layer rather than only the rim (the
demo's F2). Every `event_fired` assertion of the committed scenarios still
holds by its deadline, and the interpreter's transcripts (`interpreter/`) did
not move. `expand-east-segment`'s event log moved in its `ore_delivered` lines
alone: each seat's first load lands earlier (ticks 1430 and 1915, where they
were 2157 and 2036), because a drone now reaches the stands inside its seam's
top layer, and seat 0's second load is a full `$` 64 at 3023 where both seats'
second loads were the rim's last `$` 32. `mine-to-depth` is new: seat 0 switches its core to Mine
with `dig_max_depth` 3 every round, a `set_mandate` row whose settings the sim
now writes (item 131 (5)), and works its starting seam below the rim in all
three rounds.

**S1's grid (`grid`) moved no chain** (decisions-log item 127 (5) to (8)). Its
rulings change what the power phase does only while a seat's draw outruns its
supply or a beacon is dark, and no committed scenario before it ever had a
deficit: the Quartermaster holds fabrication at headroom and a placed beacon is
net zero (item 113 (5)). `brownout-by-recycle` is new, and it is the deficit
made on purpose (S1's plan, decision 18): seat 0 covers its vent with a
Generator in round 1, loads its grid past the core's surplus with a
low-priority Survey beacon's seven scouts in round 2, then recycles the
Generator's beacon at tick 4911, and the next settle (4912) sheds the Survey
beacon, which stays dark for the rest of the round.

**S1's Build settings (`build`) moved the four chains with a Build target and
left the rest** (S1's plan, decision 15 and decision 5; the register's S1-51).
A Build target is now its own row at `interface_times.build_target_ms`
(2.5 s) rather than one settings field at the edit's base rate (2 s), so a
`place_beacon` whose initial settings carry one Generator target commits that
row ten ticks later. The chains first differ **at the deploy tick** --
`against-easy` and `against-easy-three-rounds` at tick 691, `cover-nearest-vent`
and `brownout-by-recycle` at tick 721, the tick `beacon_placed` reports in both
runs -- because the deploy is where the interpreter sets its first initial
row's due tick, which is hashed interpreter state, and that tick is now 500 ms
further off. The row then commits ten ticks late (`row_committed` at 741 where
it was 731 in the `against-easy` pair, and at 771 in the other two), and the
payment and the build follow ten ticks late (`structure_queued` at 776 in
`cover-nearest-vent`, where it was 766; `structure_completed` at 1286 in the
`against-easy` pair, where it was 1276). A structure's rotation and a
target's order and footprint (the box an area writes, four whole-voxel columns
per row) are hashed from the same pull request (snapshot version 9), but they
are bytes only where a target or a structure exists, which these four chains
reach after the moved deploy anyway; no committed scenario writes a protected or
probe area. `deploy-and-visit`,
`expand-east-segment`, `mine-to-depth`, `round-limit-audit` and
`unaffordable-deploy` write no Build target and did not move, and neither did
the determinism chain (its harness writes none).

**S1's operator (`oper`) moved the two `against-easy` chains from their first
line (tick 1)** and no other (decisions-log items 127 (12) and 133 (3) (a); S1's
plan, decision 14). Both seats' orders changed, and neither through the sim.
Seat 0's file is Hold & Build as Easy now fills it -- `place_beacon` covering
its starting vent by name, `vent_324_16`, with walk-in in place of the walk to
a fixed site -- so its first step binds a feature on tick 1, which is hashed
seat state. Seat 1's Easy no longer places a Mine beacon beside its starting
seam, which its core covers and works: its route opens with `deepen_core` (the
core's dig depth, committed at tick 71) and then places a Build beacon covering
its own starting vent, with a Generator on it, every goal costed from the core.
Read in the events, against the chains `build` left: seat 1's beacon lands at
tick 736 (its Mine beacon landed at 446) and its Generator completes at 1495;
seat 0's beacon lands at 721 (was 691), its initial Build target commits at 771
(was 741) and its Generator completes at 1260 (was 1286); in
`against-easy-three-rounds` seat 0 first delivers ore at 1430 (as before) and
seat 1 at 1671 (was 1787), its core's drone digging from the start, and seat
0's carried deploy fails as round 2's step starts, at tick 3601 (was 4056): the
step no longer walks before it reads its site, and the vent it names is
covered by its own round-1 beacon. Seat 0's derived hold is 9 900 ms: Easy
costs the suggested vent from the core. `against-easy.scenario.jsonc`'s seat 1
deadline moved with it, from 550 to 800, and every other deadline holds. The
determinism chain did not move.

**S2's `base` moved `expand-east-segment` alone, from its first line (tick 1)**
(ruling 3 of S1's stage demo, decisions-log item 139 (1)). Its seats no longer
seal the spec's worked example: no one first site is legal both on this map and
on the verifier's reference view, so the scenario seals its own copy,
`scenarios/skeleton/expand-east-segment.playbook.jsonc`, with the first site
moved to (378, 24, 36), 20 voxels east of seat 0's core. Seat 0's walk is the
first thing that differs: `to_site` now **succeeds** for seat 0 (`step_completed`
at tick 381), `place_east` places its `b_01` at 626, the two initial rows commit
at 686 and 716, and the tail's walk to `b_01` completes where the commander
stands at 721, when seat 0's fallback engages. Seat 1, across the map, does what
both seats did before: its walk times out at 2401, its deploy is refused
`illegal_site` at 2406 and its tail `no_target` at 2411. The rest of `base`
(S2's plan, E2 to E8, E27 and E30) moved no chain: the queues are never state,
the refusals and the typed reads change nothing a committed match reaches.

**S2's `site` moved no committed chain** (S2's plan, task `site`; decisions-log
items 128 (d) and 144 (5)). Its new state -- each beacon's Build settings and
the drone work the Quartermaster has paid for, and each wall target's top -- is
hashed as a block after every other that writes nothing while every beacon
holds a placed beacon's defaults, nothing is paid and no target is a wall, and
no committed scenario writes a Build setting or a wall or fields a repair
drone. Its two scenarios are new, each one segment of 60 s on the golden seed
through the playbook door: `repair-and-salvage` writes Build's S2 settings and
builds an autocannon and a wall box at seat 0's core, and
`terraform-refills-a-crater` levels a mortar's footprint with `FILL` before it
is bought. The mortar duel the plan named for them -- repair, salvage and the
crater refill -- is headless (`crates/sim/tests/site.rs`'
`a_mortar_duel_repairs_and_salvages` and `a_mortar_duel_refills_its_craters`):
the golden seed's cores stand 335 voxels apart and a beacon stands only inside
its seat's spheres, so reaching it through playbooks took seven-beacon chains
over five rounds of 480 s, more than CI's legs could hold.

`expand-east-segment` carries one more thing worth knowing before its chain is
read. The playbook it seals **does not qualify against either seat's own frozen
snapshot** on this map: `E0401` for `b_01`, which since S1 names each seat's own
first placed beacon and neither has one when it seals, and, for seat 1, `E0403`
for a site outside every sphere of its own. No client could submit it. `gamectl scenario run` therefore seals it
through a documented harness door and prints a `note` line saying so on every
run; `deploy-and-visit` needs no such line. See `crates/gamectl/src/scenario/
run.rs` and the pull request that added the runner.

**This line format is a contract**, like `interpreter/`'s: it is the same
tab-separated event line, and a scenario file asserts on the `kind` names in it.
