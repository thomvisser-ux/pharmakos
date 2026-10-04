<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `verifier/` — reports, `report_hash`, and the catalogue

**Filled by T6** (`crates/verifier`).

* `<case>/expected.report.json` — the full report for one playbook, including
  its `report_hash`. It is the canonical `gp.api.v1.VerifyReport` JSON that
  `crates/proto`'s codec writes, which is exactly what the gateway hands a
  client back.
* `expected.catalogue.json` — the whole diagnostic catalogue, so a code cannot
  be renamed or renumbered in silence.

`report_hash` is a pure function of five inputs: playbook bytes, snapshot, rules
hash, verifier version, depth. Any of the five moving moves the hash.

## Where the inputs live

The playbook for `<case>` is `crates/verifier/tests/cases/<case>.json`, and the
producing test is `crates/verifier/tests/verifier.rs`. There is **one fixture**
for every case — one seat, one snapshot, one scope — written out in that file's
`fixture_scope` and `fixture_snapshot`:

| Beacon | Side | Writ | At | Notes |
| --- | --- | --- | --- | --- |
| `b_01` | own | BUILD | 80, 11, 55 | the seat's pre-placed core |
| `b_02` | own | MINE | 100, 20, 58 | tagged `east` |
| `e_01` | enemy, known | — | 300, 300, 40 | seen, not readable |

Sharing the fixture is deliberate: a case's job is to isolate **one
diagnostic**, and a per-case scope would make each report a function of two
things that changed instead of one.

**The map, for the cases that read it (S1).** From S1's targeting verifier the
seat's view can carry the map's vents and seams and the commander's position.
The cases named in `verifier.rs`'s `MAP_CASES` — every case that names or
describes a feature, and `w0706_a_fixed_site_by_a_covered_vent` — are checked
against `map_scope`, which is the same fixture with this added and nothing else:

| Feature | Kind | Live | Covered by |
| --- | --- | --- | --- |
| `seam_104_24` | seam, standard | yes | `b_02` |
| `seam_200_60` | seam, rich | yes | — |
| `vent_150_25` | vent, standard | yes | — |
| `vent_40_40` | vent, lean | **no** (lost) | — |
| `vent_90_20` | vent, lean | yes | `b_01` |

and the commander at 82, 13, 55. Every other case keeps the plain fixture,
because `crates/gamectl/src/seat.rs`'s reference seat is that fixture written
a second time and `gamectl verify`'s test compares the worked example's
`report_hash` with the golden here. A case is named after the code it isolates,
and a code that can be wrong in more than one shape gets more than one case —
`E0111` has three, because an unset choice on `Fallback.posture`, on
`Location.place` and on `BeaconRef.ref` are three different things for an author
to have done. Three cases are meant to pass with no error:

* `expand_east` — spec section 10's worked example, canonicalised. It must
  qualify and measure **6 size units** (decisions-log item 94's own worked
  example). From S1 it carries four `I0003` notes and nothing stronger: it
  omits `pace` on two moves and `seam_choice` and `pillar_spacing` on its Mine
  block, and S1's plan's decision 11 (the register's S1-39, ruled by item 128)
  reads each as a named default with a note. If it ever gains a warning or an
  error, either the verifier or the spec's example is wrong, and the pull
  request has to say which.
* `cover_the_nearest_vent` — targeting's adopted spelling, "place a beacon
  covering the nearest vent you do not cover, and build a Generator on it",
  against the map: no diagnostic at all.
* `budget_128` — a playbook that sits **exactly on** the size budget, for a
  walled bench harness to time later. Wall-clock time is illegal in
  `crates/verifier`, so nothing here times anything: the QUICK and FULL timings
  against this fixture are **S1's P1 gate's**, which builds the walled harness
  that measures them and sets their budgets (decisions-log item 116 (6)(b);
  AGENTS.md section 9 item 11). No producer exists in the walking skeleton.

## One `path` that is not a node pointer

`gp.api.v1.Diagnostic.path` is an RFC 6901 JSON Pointer into the playbook, and
every code here carries one — except `E0001` in its **lexical** form. When the
text is not JSON at all there is no tree to point into, so the codec reports a
byte offset and spells it `/byte/583`; the verifier forwards that verbatim
rather than rewriting it to the root, because the offset is the useful answer.
An editor branches on the shape: a first token of `byte` is an offset into the
bytes it sent, anything else is a node pointer. `e0001_not_json` is that case.

## Patch suggestions are `add`, not `replace`, wherever the member may be absent

RFC 6902 section 4.3 makes a `replace` fail unless its target already exists,
and proto3 JSON omits a field sitting at its default — so a playbook whose
`hold` has no `ms`, whose `wait_until` has no `timeout_ms` or whose handler has
no `cooldown_ms` has no such member for a `replace` to land on, and those are
exactly the files the fixes are offered on. Section 4.1's `add` creates the
member when it is absent and replaces the value when it is not, so it is right
in both directions. `replace` survives only where the diagnostic itself proves
the member is there (a `max_fires` of 9, a negative duration, a percent above
100). A suggestion that flips between the two ops is a behaviour change like any
other and the pull request says which way and why.

## Regenerating

```sh
cargo test -p pharmakos-verifier        # writes <target>/golden/verifier/**/actual.*
cargo xtask golden --bless              # accepts them
```

## What a diff means

* **A `report_hash` moved and the report did not.** One of the other four inputs
  moved — most often the rules hash or the verifier version. Say which.
  `rules_hash` covers the **whole** rules table including its `note`
  (decisions-log item 89), so a tuning pull request moves every report in this
  directory, and that is correct rather than surprising. The **snapshot** is the
  other one that moves in bulk: a sim task that adds hashed state widens
  `Snapshot`, the fixture snapshot's bytes change, and every report hash here
  moves with them while the reports themselves stay identical. T14 is the first
  time that happened.
* **A report moved.** A diagnostic's code, severity, JSON Pointer, message,
  beginner sentence or patch suggestion changed. The pointer and the code are
  what clients bind to; the message is what players read.
* **The catalogue moved.** Code numbers are **append-only from the day they
  ship**. A renamed or renumbered code is a breaking change to every saved
  report and every piece of documentation that quotes it. A code *added* at the
  end of its family is ordinary; anything else needs the pull request to say why.
  Each row carries its `emitter`: a stage name, or `none at the skeleton (…)`
  naming the task that owes it. A row moving from `none…` to a stage name is a
  code starting to fire, and the case directory for it should arrive in the same
  pull request.
* **A case appeared or vanished.** `crates/verifier/tests/verifier.rs` asserts
  that every catalogue row with an emitter is produced by some case and that no
  case produces a code the catalogue does not hold, so the set of directories
  here is the coverage claim.
* **Reports differ between operating systems.** They must not — byte-identical
  across the three is one of T6's acceptance tests. That is a determinism hole
  in the verifier, not a report change.
* **FULL's report grew when S1 and S3 fill the estimate and lint stages.**
  Expected, and written down in advance (decisions-log item 82): explain it in
  that pull request. S1's targeting verifier was the first: the lint stage
  raises `W0704` to `W0706`, so the `w07…` cases' FULL reports carry a warning
  their QUICK reports do not. `full_finds_what_quick_finds` holds in its S1
  form: FULL's diagnostics start with QUICK's, and what follows comes only from
  a FULL stage and is never an error.
* **Every `report_hash` moved in S1's targeting verifier, the reports whose
  diagnostics stayed the same included.**
  The seat's view widened to carry the features and the commander's position
  (`Scope::encode`), and `REPORT_HASH_DOMAIN` went from `gp.api.v1/report/1` to
  `/2` with it, so input 2's bytes moved for every case, the cases with no map
  included (an empty feature list and an absent commander are bytes too).
