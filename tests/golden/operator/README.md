<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Operator goldens

What the built-in operator **seals**, written down: the playbook Easy submits
for every seat of a three-seat match in its first Lull, per seed. Produced by
`the_operator_is_deterministic_for_a_given_match_seat_round` in
`crates/gamectl/tests/operator.rs`, which hosts the match through the same
`Host::open_from`, `Surface` and `serve::InProcessSeats` that `gamectl host`
uses, with Easy playing every seat through its own in-process token; compared
by `cargo xtask ci`'s `golden` step, and so by the three-OS matrix; re-blessed
with `cargo xtask golden --bless`, which a pull request then has to explain
(AGENTS.md §5).

Spec section 14 makes the operator deterministic -- "its seed comes from the
match, seat and round" -- and these files are that claim made checkable: the
test asserts two hosts of the same match seal the same bytes, and the golden
step asserts those bytes are the same on Windows, Linux and macOS.

## The cases

| Case | Files | What it pins |
| --- | --- | --- |
| `seed-0102030405060708/` | `expected.seat-0.jsonc`, `expected.seat-1.jsonc`, `expected.seat-2.jsonc` | The determinism golden's seed (`pharmakos_sim::DETERMINISM_MATCH_SEED`), three seats, round 1 |
| `seed-00000000ca5caded/` | the same three | The scenario files' seed, three seats, round 1 |

Each file is the seat's sealed playbook exactly as `submit_plan` accepted it.
A **composed** plan is one of the library's templates, filled through
`patch_plan` (so the template's comments and layout survive), with
`meta.note` saying what Easy chose and why, opening with the seed it recorded
and did not use (decision C15). A seat with nothing inside its route share
seals its own **safe playbook** instead: the Safe Playbook template
instantiated with nothing raised, whose `meta.note` is the template's own and
carries no seed (Easy records the seed of that round in its "why", which is
not part of the playbook).

## What a diff means

A diff here is a change in **what the built-in opponent does in its first
Lull**. It moves when any of these moves, and the pull request says which:

* **the operator** (`crates/operator`): its candidates, its utility, its
  composition, its "why" wording;
* **the templates** (`library/`), which every playbook here is filled from;
* **the rules text** (`rules/rules.v1.json`) rows it scores with -- costs, kW
  ratings, yields, interface times, the sphere radius;
* **what the wire shows a seat**: the map (the generator), the beacons and the
  commander (`list_beacons`, `get_view`), the economy
  (`get_economy_forecast`), and travel (`estimate_route`). A change to the
  view's fog rule, for instance, moves it only if it changes what a seat sees
  in its own first Lull;
* **`patch_plan`'s layout** of a replaced or inserted value (plan-core);
* **the producing test's own match settings** in
  `crates/gamectl/tests/operator.rs`: three seats, a 60 s segment
  (`SEGMENT_MS`, not the rules table's segment lengths), no units, three
  rounds. Easy fills 70 % of the segment, so a different segment length
  changes which goals fit and moves every file.

A diff that appears on one operating system and not the others is a
determinism bug, in the operator or in something it reads, and is never
re-blessed away.
