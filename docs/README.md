<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The docs tree

Two kinds of document live here, and they are not equal.

- **`spec/`** is what the game *is*. One current specification, superseding everything else.
- **`design/`** is how it got there: the decisions, their reasoning, the investigations
  behind them, and the handoffs between working sessions. It is a record, not a contract.

**Precedence.** The specification is what the code is built against, and it wins over every
older design document. The one exception is a decision the spec has not caught up with yet:
`design/decisions-log.md` §2.7 is the newest record, so where it is newer than the spec
text, it wins and the spec is what needs fixing. Inside the log, later sections override
earlier ones (`2.7 > 2.6 > 2.5 > 2.4 > 2.3 > 2.2 > 2.0 > 2.1`), and within §2.7 a later item
overrides an earlier one. `design/README.md` carries the full precedence order; read it
before taking a rule from any design document.

Docs are MIT OR Apache-2.0, like the schemas — the game code is GPL-3.0-or-later.

## `spec/`

| File | What it is |
| --- | --- |
| `pharmakos-spec-v0.6.html` | **Current.** Draft 6, dated 2026-09-13. Sections 1–19: vision, world, match structure, commander, beacons, mandates, economy, capabilities, maps, playbooks, verifier, Seat Gateway, editor, built-in operator, **§15 architecture**, spikes and gates, the build plan to v0.1 and v1, roadmap, tuning and risks. |
| `airgap-spec-v0.5.html` | Archive. Draft 5, under the AirGap codename. Superseded — kept so decisions that cite it stay readable. |

The sections a coding agent needs most often: **§15** for crate boundaries, determinism,
maths, licences and the reserved seams; **§16** for the spike and gate criteria; **§17** for
the stage order and what each stage's definition of done is.

## `design/`

| File | What it is |
| --- | --- |
| `README.md` | The index of this folder and the full precedence order. Start here. |
| `decisions-log.md` | The primary record. Every decision with its reasoning, from the pre-spec interview through pass 6 (§2.7, items 1–45 are current). Read the precedence note above before quoting it. |
| `handoff.md` | Session-to-session state: what is published where, what is open, what to do next. Read this first when picking the project back up. |
| `co-design-gameplan-api.md` | Co-design of the playbook format, the verifier, the planning API and the editor, with its own open-questions and contradictions sections. |
| `6c-language-investigation.md` | The multi-agent investigation into a shared plan format and script language, which ended in the v1 simplification: JSONC data, no script runtime in the game. |
| `review-agenda-draft5.md` | The draft-5 review agenda that drove the draft-6 rewrite. Historical. |

## Also in `docs/`

Written by their own assignments, not by this index, and listed here so the tree reads
whole: `CONTRIBUTING.md` and `DCO.txt` (how to contribute and the sign-off), `LICENSING.md`
(which licence covers which directory, and the REUSE manifest), and `spikes/` (the stack
spike write-ups — G4 determinism first).

## Where new documents go

- A change to the rules of the game belongs in the **spec**, not in a new document.
- A decision and its reasoning belongs in **`design/decisions-log.md`**, appended, never
  rewritten in place.
- Anything generated — schema docs, the diagnostic catalogue, `llms.txt` — is produced from
  the Protobuf schema by `gamectl docs` so it cannot drift, and is not hand-written here.
- Developer-facing instructions (`AGENTS.md`, `CLAUDE.md`) live at the repository root, not
  in this tree; they arrive with harness part 1.

Contract files — `.proto`, the lints, determinism code — need owner approval to change.
That rule covers the documents that define them too.
