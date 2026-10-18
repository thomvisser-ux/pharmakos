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
  turtle.scenario.jsonc
  hunter.scenario.jsonc
  placeholder.playbook.jsonc          what the three seal until S2's `arms` lane
  fixture/result-golden.scenario.jsonc  the arm the result golden is played from
  fixture/two.seeds.txt                 and its two seeds
```

**The arms are placeholders until S2's `arms` lane.** Each seals
`placeholder.playbook.jsonc`, which covers the nearest vent and attacks
nobody, so a win rate today says nothing about balance.
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

The band (decision 18): an arm that is the sole winner in more than 13 of the
20 seeds (65 %) against Easy opens a tuning task. It is an alarm, not a gate:
`.github/workflows/nightly-scenarios.yml`'s band step warns and fails nothing.

## The seed set

`seeds.txt`: one seed per line, `0x` and exactly sixteen lowercase hex digits
(a scenario's own `map.seed` spelling), `#` comments, blank lines skipped. The
decoder caps the file at 64 KiB, a line at 256 bytes and the set at 1 024
seeds, and refuses a malformed line or a repeated seed with its line number
(`crates/gamectl/src/scenario/seeds.rs`, whose property test is
`crates/gamectl/tests/seeds.rs`). Its first ten seeds are also G4's bar: the
workflow's `g4` job plays ten seeded matches of 9 600 ticks on each of the
three operating systems and byte-compares their chains. A seed changed here
is a new night, and the pull request that changes it says so.
