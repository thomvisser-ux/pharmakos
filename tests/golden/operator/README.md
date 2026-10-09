<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Operator goldens

Two kinds of case. What the built-in operator **seals**, and what it
**suggests** to a human seat's wizard.

## What it seals

The playbook Easy submits for every seat of a three-seat match in its first
Lull, per seed. Produced by
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
and did not use (decision C15). Since S1's targeting every target Easy
writes is a **name** -- `{"feature_id": "vent_52_335"}` where the template's
description stands -- and the route opens with `deepen_core`, the step that
gives the core's Mine settings a `dig_max_depth` (decisions-log item 133 (3)
(a)), inserted by the patch's last operation as the route's first element, so
it lands however the rest of the route runs. Its walk and interface time are
paid out of Hold & Build's hold, and a hold with nothing left for it is taken
out; the visit goals keep the whole share. A seat with nothing
inside its route share seals its own **safe playbook** instead: the Safe
Playbook template instantiated with its route -- the walk to safety, any
priority raise, and the same `deepen_core`, last -- whose `meta.note` is the
template's own and carries no seed (Easy records the seed of that round in
its "why", which is not part of the playbook).

## What it suggests: `wizard/`

| Case | Files | What it pins |
| --- | --- | --- |
| `wizard/expand_and_mine/`, `wizard/hold_and_build/`, `wizard/safe_playbook/` | `expected.playbook.jsonc`, `expected.parameters.json`, `expected.report.json`, `expected.prose.txt` | The wizard's live answer for the human seat of a match hosted as the lobby hosts it (seed `0x00000000ca5caded`, two seats, the rules table's own segment ladder), with the real Easy advising it |

Produced by `the_wizards_live_suggestion_qualifies_and_is_goldened_for_every_template`
in `crates/gamectl/tests/wizard.rs` (decisions-log item 124 (5) (l)): for
each template, `instantiate_template{suggested: true}`'s playbook byte for
byte, its declared parameters with the values applied and whether each is
the operator's, and Easy's "why"; then that playbook's FULL `verify_plan`
report (which must qualify) and its `render_plan` prose. The gateway's own
`instantiate_suggested` golden answers the same call from a scripted
advisor; this is the one with the real operator. No `_status` footer is
written: it carries the host clock, which is the lobby's.

## What a diff means

A diff here is a change in **what the built-in opponent does in its first
Lull**, or in **what it tells a human to do**. It moves when any of these
moves, and the pull request says which:

* **the operator** (`crates/operator`): its candidates, its utility, its
  composition, its "why" wording;
* **the templates** (`library/`), which every playbook here is filled from;
* **the rules text** (`rules/rules.v1.json`) rows it scores with -- costs, kW
  ratings, yields, interface times, the sphere radius;
* **what the wire shows a seat**: the map's named features
  (`get_map_summary.features`), the beacons and the commander
  (`list_beacons`, `get_view`), the economy (`get_economy_forecast`), travel
  and the covering site (`estimate_route`), and whether an `on` reads
  (`resolve_refs`). A change to the
  view's fog rule, for instance, moves it only if it changes what a seat sees
  in its own first Lull;
* **`patch_plan`'s layout** of a replaced or inserted value (plan-core);
* **the producing tests' own match settings**: in
  `crates/gamectl/tests/operator.rs` three seats, a 60 s segment
  (`SEGMENT_MS`, not the rules table's segment lengths), no units, three
  rounds; in `crates/gamectl/tests/wizard.rs` the lobby's two seats, the
  rules table's own segment ladder and three rounds. Easy fills 70 % of the
  segment, so a different segment length changes which goals fit and moves
  every file;
* for `wizard/`'s `report.json` and `prose.txt`, the verifier and the
  renderer as well (`report_hash` moves with `VERIFIER_VERSION` and the
  snapshot, as `tests/golden/verifier/`'s do).

A diff that appears on one operating system and not the others is a
determinism bug, in the operator or in something it reads, and is never
re-blessed away.
