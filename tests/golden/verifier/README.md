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
things that changed instead of one. A case is named after the code it isolates,
except the two that are meant to pass:

* `expand_east` — spec section 10's worked example, canonicalised. It must
  verify clean and measure **6 size units** (decisions-log item 94's own worked
  example). If this case ever gains a diagnostic, either the verifier or the
  spec's example is wrong, and the pull request has to say which.
* `budget_128` — a playbook that sits **exactly on** the size budget, for a
  walled bench harness to time later. QUICK's p99 is "measured and reported, not
  gated" (skeleton plan T6), and wall-clock time is illegal in
  `crates/verifier`, so nothing here times anything: **T20's walled harness
  does**, against this fixture.

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
  directory, and that is correct rather than surprising.
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
  that pull request. Until then `full_finds_what_quick_finds` holds, and the two
  depths differ only in the depth stamped on the report and hashed into it.
