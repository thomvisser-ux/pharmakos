<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Design documents — index and precedence

Pharmakos was designed before it was built, across six interview passes. The documents disagree with
each other in places, because each pass overrode the one before it and the older text was kept for
its reasoning. **Read the precedence order before you take a rule from any of them.**

## Precedence

1. **`decisions-log.md` §2.7** — pass 6 (2026-09-13), the v1 simplification. Items 1–45 are current
   and override everything else, including the spec where the spec has not caught up. Within the
   file the order is **§2.7 > §2.6 > §2.5 > §2.4 > §2.3 > §2.2 > §2.0 > §2.1 > §1**.
   Two wordings inside §2.7 are themselves superseded by later items in the same section: item 7's
   "v0.1 at about wk 19" is replaced by item 31 (v0.1 is the gated build at wk 35.5), and item 15's
   "nightly league" by item 34 (three nightly adversarial scenarios).
2. **The spec, `../spec/pharmakos-spec-v0.6.html`** — draft 6, the readable whole. Authoritative for
   anything §2.7 does not settle. Nineteen sections; section 15 (architecture), 16 (gates) and 17
   (build plan) are what the harness implements against.
3. **`co-design-gameplan-api.md`** — the playbook / verifier / gateway / editor co-design. The
   deepest normative detail anywhere (proto sketch, condition vocabulary, diagnostic catalogue,
   interface-time model), but it predates the v1 simplification. **Superseded wherever §2.7
   conflicts** — notably: the 27 MCP tools, the preempt flag and handler stack, `reflex_hp_pct`,
   `branch`/`repeat` in v1, the Dispatch field, `stop_at_pool`, and the WASM planner. Use it for
   shape and detail, never as the last word on scope.

If a rule appears in none of the three, it is not a rule. Raise it with the owner rather than
inventing it (`AGENTS.md` §12).

## The documents

| File | What it is | Status |
|---|---|---|
| `decisions-log.md` | Every decision from the interview passes, in sections. **§2.7 (items 1–45) is current.** | **Authoritative** |
| `../spec/pharmakos-spec-v0.6.html` | Design specification draft 6 (v0.6, 2026-09-13), 19 sections, published. | **Authoritative** below §2.7 |
| `co-design-gameplan-api.md` | Playbook schema, verifier, gateway API and editor UX, co-designed in depth. | Normative in shape; superseded where §2.7 conflicts |
| `handoff.md` | State of the design at the end of pass 6: what is decided, what is next, working conventions, the build plan in one line. Start here for orientation. | Current |
| `6c-language-investigation.md` | The 36-agent investigation into the plan format and script language that produced the v1 simplification (JSONC now; TypeScript-family scripts run externally in v1.1; no embedded runtime). | Input to §2.7; roadmap material for v1.1 |
| `review-agenda-draft5.md` | The draft-5 review agenda: 30 items, the editorial fixes applied, and the moot and rejected lists. | Historical |
| `../spec/airgap-spec-v0.5.html` | Draft 5 of the spec, kept for diffing against draft 6. | Archive — do not implement from it |

## The short version

v1: everything that executes is Rust — the commander's **operator**, the beacons' **mandates**, the
units' and buildings' **programs**. Everything the player touches is data: **playbooks** and
templates as JSONC (canonical `gp.v1` proto JSON plus comments). No script runtime, no MCP, no SDK,
no published API, no AI seats, no manual control. The Seat Gateway is internal but agent-shaped;
script seams are reserved in the proto and never read. The sim is deterministic at 20 Hz on integer
maths, hashed every tick. Build plan: 63.5 weeks, one gate at week 35.5 that also ships v0.1
publicly, v1 at week 63.5.

Implementation rules — determinism, crate boundaries, contract files, security, commits, CI, the
definition of done — are in [`../../AGENTS.md`](../../AGENTS.md), not here. Contribution mechanics
are in [`../CONTRIBUTING.md`](../CONTRIBUTING.md).

## Changing a design document

These files are contract files (`AGENTS.md` §5). A design change is not a refactor: open a PR that
states what decision changed and stop. New decisions are logged in `decisions-log.md` as they are
made — continue §2.7 or open §2.8 for the next pass — and the spec is updated to match, never the
other way round.
