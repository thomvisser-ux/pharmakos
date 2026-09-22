<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `determinism/` — the per-tick state-hash chain

**Filled by T2** (`crates/sim`), extended by every later sim task.

`expected.hashes.txt` is one line per simulated tick, in tick order from 0:

```text
<tick in decimal>    <xxh3-64 state hash as 16 lowercase hex digits>
```

The separator is a TAB. LF endings, a trailing newline, no carriage return
anywhere — `.github/workflows/ci.yml` uploads this file per operating system and
a later job byte-compares the three. That comparison is gate G4 in the flesh:
identical hashes on three operating systems. The format is enforced line by line
by `validate_hash_file` in `xtask/src/main.rs`.

## What the committed chain covers (T11)

**A sealed playbook, running.** Every seat seals
`pharmakos_sim::determinism_playbook` — a harness playbook whose every target is
a **selector** rather than a voxel, so the same file is legal for every seat on
every spawn — and the chain therefore covers the interpreter's hashed state: the
route cursor, the stage of the step in progress with its start tick and
deadline, the pinned selector target, the visit's row and its commit tick, the
reflex's marker, and the handler's fire count and cooldown.

The harness route also **deploys a beacon**, which is the one thing a tick does
that adds a row to a table: `crates/sim/tests/allocations.rs` counts the ticks
around that deploy separately, so `BeaconTable::reserve` is asserted over a
deploy that actually happens rather than described in a doc comment. It is why
the beacon table's length is not the same on every tick of the chain.

Two consequences worth knowing before reading a diff:

* A **commander's** destination is the playbook's to set. The harness's random
  destination draw (`Stream::Spawn`) still moves the `units_per_seat` walkers and
  no longer reaches a commander, so a seat that sealed nothing leaves its
  commander standing where the generator put it.
* A decision falls on every fifth tick of a Push — `match.decision_tick_ms` is
  250 ms — starting with the Push's first played tick. A diff that begins on a
  tick that is not `1 mod 5` of a segment's start is *not* a decision moving.

## What the committed chain covers (T10)

**Whole segments, and the phase changes between them.** One line per *Push*
tick: a Lull and a recap consume no tick, because in the sim they are states
rather than durations (the Lull's timer is a host concern the sim never reads —
AGENTS.md §4.5). The determinism harness plays the host's part, opening the
Push the moment a Lull is reached and closing the recap the moment one opens.

The harness's own per-round length list is `DETERMINISM_SEGMENT_LENGTHS_MS` in
`crates/sim/src/lib.rs` — 20 000 ms then 15 000 ms, so 400 ticks then 300 —
chosen so that the 1 200-tick run covers `push → recap → lull → push` three
times rather than sitting inside the first three-minute Push of item 68's real
ladder. It is a **host** setting (item 40 makes the per-round list one), it is a
PLACEHOLDER, and it goes when `DETERMINISM_TICKS` is raised to a real segment at
T20.

So a diff that starts exactly at tick 400, 700 or 1 000 is a segment-boundary
change — the close, the round increment, or the coming segment's length — and a
diff that starts at tick 0 is a change to what the encoding covers.

## What the committed chain covers (T14)

**The economy.** The encoding gained the `$` and `kW` state and the tables the
Quartermaster, the mandates and Survey write, so the chain now covers all of it:

* the seat row's **Quartermaster round-robin cursor**, which decides whose turn
  it is within an urgency band;
* the unit row's **home beacon**, what it is **carrying** and the tick it is
  **busy until** — a mining drone's load and a build drone's work;
* the beacon row's **Quartermaster priority** and its **scout count**;
* the structure row's **building** flag, which is what "paid means yours"
  distinguishes from finished;
* three blocks that did not exist: the beacons' **mandate targets** (Build
  targets, protected areas, Survey probe areas), the seats' **sightings** with
  the tick each was taken at, and the per-asset **kill-credit** damage counters.

Because the seat and unit rows are wider from the first tick, **the T14 chain
diverges from the T13b chain at tick 0** and every line after it. That is the
encoding covering more, not the sim walking differently.

## What a diff means

**The sim's behaviour changed.** That is all it can mean: the chain is a pure
function of (map seed, playbooks, rules hash), so nothing else moves it.

* **You changed behaviour on purpose.** Say so in the pull request — which rule,
  which tick the chain first diverges at, and why. Then `cargo xtask determinism
  --bless`, **not** `cargo xtask golden --bless`: this chain is compared by the
  `determinism` step, which runs the sim and writes the fresh chain to
  `<target>/determinism/hashes.txt`, so the `golden` step leaves this area alone
  (`SELF_COMPARED_AREAS` in `xtask/src/golden.rs`). AGENTS.md §5: never re-bless
  to get a red build to green without that explanation.
* **You added a field to sim state.** AGENTS.md §4.8: a new field goes into the
  state hash, the snapshot/restore round trip **and** the goldens in the same
  pull request. A field that affects behaviour and is not hashed is a latent
  desync that only surfaces when two operating systems disagree.
* **It moved on one platform only.** Not a behaviour change — a determinism
  hole. Bisect by tick from the first differing line. The usual suspects are
  unordered iteration, an unchecked cast, a non-total sort comparator, a float
  that crossed the wall, and `usize`/`isize` reaching hashed state.
* **It moved and you changed nothing but a performance knob.** Then the knob is
  inside hashed state and should not be (G3' §9.17's calibration-constant
  lesson). Fix that, not the golden.
* **It moved and you changed only the event bus.** Then the bus has become
  hashed state, and it must not be: events are derived output that nothing in a
  tick reads (`crates/sim/src/events.rs`). Fix that, not the golden.
