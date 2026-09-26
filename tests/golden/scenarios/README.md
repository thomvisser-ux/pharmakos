<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `scenarios/` — what the scenario runner asserts on

**Filled by T11** (the first full-segment hash chain) and T15 (the runner).

One directory per scenario, named after its `name` key:

```text
tests/golden/scenarios/<name>/expected.hashes.txt   the per-tick chain
tests/golden/scenarios/<name>/expected.events.txt   the event log its producing
                                                    test writes (expand-east-
                                                    segment only)
```

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
its core's sphere and delivers it twice over the segment, and the Ledger settles
at the recap. Those `ore_delivered` and `settled` lines come from the mandate
layer rather than from the playbook, which is the point — a seat that seals
nothing still earns and is still paid.

**T15 added it: `deploy-and-visit`.** Four route steps and all four complete —
a walk, a deploy that puts a beacon in the ground at tick 451, a walk home and
an interface row committed on the core at 776 — with seat 1 sealing nothing so
that the gateway files the safe playbook for it. Read the two together: this
one is what the interpreter does when a playbook fits the map, that one is what
it does when a playbook does not, and neither is the whole picture on its own.

**T14b moved both chains from their first line (tick 1)** and nothing else
here: the key-core rule (decisions-log item 113 (5)) changed every seat's
settled `kW` draw, which is hashed seat state, and changed no event these
scenarios report. `expand-east-segment`'s event log did not move, and every
tick `deploy-and-visit` asserts on or quotes (206, 451, 511, 716, 776) is where
it was. That is the "chain moved but the event log did not" case above, read
and accepted: the scenarios assert on the route, not on the meter.

`expand-east-segment` carries one more thing worth knowing before its chain is
read. The playbook it seals **does not qualify against either seat's own frozen
snapshot** on this map: `E0401` for `b_01`, which is seat 1's core and is not in
seat 0's view, and `E0403` for a site two hundred and sixty voxels outside every
sphere. No client could submit it. `gamectl scenario run` therefore seals it
through a documented harness door and prints a `note` line saying so on every
run; `deploy-and-visit` needs no such line. See `crates/gamectl/src/scenario/
run.rs` and the pull request that added the runner.

**This line format is a contract**, like `interpreter/`'s: it is the same
tab-separated event line, and a scenario file asserts on the `kind` names in it.
