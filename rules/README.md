<!-- SPDX-FileCopyrightText: 2026 Pharmakos contributors -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->

# The rules table

`rules.v1.json` is the canonical JSON of one `gp.v1.RulesTable`
(`proto/gp/v1/rules.proto`). It is decisions-log **item 78**: every tuning
value the sim, the verifier, the estimator and the mesher read lives in one
reviewable file, loaded by `pharmakos-sim` and hashed with item 48's encoder,
so `rules_hash` — one of the verifier's five inputs — covers all of it by
construction.

The split that makes this work:

- **The shape is a contract.** It is a `.proto` message, so `buf breaking`
  guards it in `WIRE_JSON` mode and a renamed row is caught the way a renamed
  playbook field is. Changing the shape needs owner approval (AGENTS.md §5).
- **The values are data.** A tuning change during S1 or S2 is an ordinary PR.
  It moves `rules_hash`, and therefore every golden hash chain and every
  `report_hash`, so the PR that moves it explains the movement (AGENTS.md
  §10 item 2) — but it is not a contract change.

This is what AGENTS.md §12 asks for in so many words: *tuning values are data,
versioned in the rules table and stamped into the rules hash — not constants
sprinkled through the code.*

## What is NOT in here

Anything that varies with the machine rather than with the game. G3′'s
calibration-constant lesson is that a performance knob inside hashed state is a
desync waiting for a slower laptop.

Two rows sit right on that line and are here deliberately:

- **the mesher's K and B.** A per-frame presentation budget — but item 54 makes
  the *drain order* a rule, so the whole block is data the mesher reads rather
  than constants it holds.
- **the per-tick repath cap.** Item 60 makes the cap part of the sim's
  behaviour: it decides which unit repaths on which tick.

Neither is a calibration constant, and the test is the same for both: the value
is the same number on every machine.

## Where the numbers come from, and which are still guesses

| Row | Source | Settled? |
|---|---|---|
| `match.segment_lengths_ms` 3 / 5 / 8 min | item 68 | yes |
| `match.lull_ms` | — | **PLACEHOLDER** — Tuning, owner, at the skeleton's demo |
| `locomotion.step_cost_*`, `climb_surcharge`, `move_cost_per_tick` | item 59 | 10 / 14 / 3 yes; `climb_surcharge = 4` has **no gameplay evidence behind it** (item 59) — owner, S3 |
| `locomotion.repath_cap_per_tick` | item 69 | **PLACEHOLDER** — 16 is provisional, re-derived at S2's exit once the burst frequency is a measurement |
| `locomotion.fog_cost_*` 3 / 2 | item 61 | yes |
| `locomotion.hpa_cluster_voxels` 32 | item 58 | yes |
| `broadphase.cell_size_voxels` | item 67's caveat | **PLACEHOLDER** — Tuning, tied to unit density, owner at S2's exit. 16 is the cell edge spike G3′ actually measured with, and no more than that |
| `interface_times.*` | spec section 5's table | values are the spec's; all of section 5 is marked Tuning |
| `mesher.surfaces_per_frame` (K), `bytes_per_frame` (B) | item 54 | **PLACEHOLDER** — no measured frame-time reason separates K = 4 from K = 8 on the spike machine; owner, S6 art pass |
| `mesher.age_frames` | item 54 | **PLACEHOLDER** — ships untested by measurement and is labelled insurance |
| `mesher.light_max`, `light_atten` | — | **PLACEHOLDER** — Tuning, owner at S6's art polish |
| `economy`, `power` | — | **PLACEHOLDER STUB** — empty. Every `$` and `kW` row is T14's, with the numbers decision 14 settles |

## Editing it

Write the JSON, then let the codec canonicalise it — field-number order, two
spaces, one entry per line — so the diff of a tuning change is one line:

```sh
cargo test -p pharmakos-proto            # writes target/golden/proto/actual.rules.v1.json
cargo xtask ci                           # the `golden` step compares it
```

`crates/proto`'s `the_rules_table_is_in_canonical_form` test fails if the file
is not already canonical, and names the first byte that differs.
