<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `interpreter/` — decision and step transcripts

**Filled by T11** (`crates/sim`: the playbook interpreter at the v1 vocabulary).

One committed playbook per vocabulary construct, each with a pinned transcript
of decisions (every 250 ms of game time), step transitions, and the points at
which an interface row commits.

## The layout

```text
tests/golden/interpreter/<case>/playbook.json           the committed playbook
tests/golden/interpreter/<case>/expected.transcript.txt the pinned run of it
```

`playbook.json` is canonical `gp.v1` JSON — the same bytes the gateway would
submit, minus the comments a `.jsonc` file may carry — and it is read by
`crates/sim/tests/interpreter.rs`, which compiles it, seals it for seat 0 of the
determinism seed's three-seat map and plays the segment.

A transcript is one **event** per line, and nothing else:

```text
<tick>\t<seq>\t<kind>\t<seat>\t<subject>\t<value>
```

`kind` is the assertable name a scenario file uses (decisions-log item 97),
`subject` is a raw `AssetId` (or `-`), and `value` is the kind's own number —
a step index, a row index, a failure reason, a resume. **Positions are
deliberately absent**: a transcript is about what the interpreter decided, and
the world's own movement is pinned by `determinism/`.

**This line format is a contract**, as is the vocabulary of `kind` names: a
scenario file asserts on both. One thing it does not carry: a `step_*` line's
`value` is a route index *or* a rule-body index, and the line does not say
which — a reader tells them apart by the `rule_fired` / `rule_ended` pair a body
runs between. Splitting them (a `rule_step_started` kind, or a second field) is
additive and waits for an assertion that needs it (owner, at T15).

The transcript also pins the **decision cadence**. Every interpreter line falls
on a tick one more than a multiple of five, because `match.decision_tick_ms` is
250 ms and a tick is 50 ms; a cadence that moved would move every tick of every
case. The runner's own lines (`match_started`, `lull_opened`, `push_started`)
and the host's `plan_sealed` are not decisions and are not on that clock.

## The cases

| Case | What it pins |
| --- | --- |
| `visit` | `move` to a late-bound selector, then `interface`: the visit handshake (1.5 s), a mandate switch (8 s) and a priority row (1.5 s), each committing at the end of **its own** duration, then the fallback |
| `place_beacon` | `place_beacon` with a 12 s deploy and initial settings at interface rates, then an `interface` with the beacon it just deployed, named by the `b_NN` id, and a `shadow` fallback |
| `guards` | `skip_if`, a `timeout_ms` that fires, `on_fail: JUMP_FORWARD` over a step, a `broadcast` refused with `no_mast`, and `on_fail: ABORT_ROUTE` into a `patrol` fallback |
| `handlers` | four handlers in priority order; the `all`, `any` and `not` tree shapes; the `rule_fired` predicate; cooldowns; **all four** `resume` values (`CONTINUE`, `SKIP_STEP`, `JUMP_FORWARD`, `END_ROUTE`); a `max_fires` of 2 driven to its ceiling; and a handler firing *after* the fallback has engaged |
| `reflex` | the fixed 20 % reflex aborting a visit, the committed row staying, the resumed visit paying the handshake again, and the reflex **not** re-firing until new damage arrives |

The `reflex` case is the one that needs something the playbook cannot ask for:
damage. `script()` in `crates/sim/tests/interpreter.rs` applies it at named
ticks, and that list is part of the case.

## What a diff means

* **Execution order changed.** One rule body at a time, no pre-emption, and the
  fixed 20 % reflex as the only interrupt. A transcript that reorders is either
  a real rule change or a scheduling accident; there is no third option.
* **A commit point moved.** Interface rows commit at the end of their own
  durations, at spec section 5's rates, and a resumed visit pays the handshake
  again. A moved commit point usually means a rate moved in the rules table —
  say which row. The rates are `interface_times.*` and they are read on every
  commit, never cached in the compiled plan.
* **The reflex fires more or less often.** It is not disableable, it aborts a
  visit, it walks to the safest own beacon, and it re-fires only after *new*
  damage. A transcript with two consecutive reflexes and no damage between them
  is a bug, not a golden to bless.
* **A step that used to time out now does not.** Every `wait` has a timeout and
  every jump goes forward only. A transcript change here is a termination
  property changing, which is the one thing `every_playbook_halts` exists to
  hold.
* **A line's tick is no longer `1 mod 5`.** The decision cadence moved, or
  something outside the decision phase started emitting an interpreter event.
  Both are behaviour changes and neither is a formatting difference.
* **A kind changed its name.** A name is append-only from the day it ships
  (item 97): every scenario file that asserts on it breaks. Add a kind; do not
  rename one.
