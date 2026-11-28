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
| `s1-plan.md` | S1, the economy: the tasks, lanes, waves, re-bless ledger, P1 and the owner's decisions, all ruled (decisions-log item 128). | Closed with S1 (decisions-log item 143, 2026-10-16); its rulings stand |
| `s2-plan.md` | S2, destruction and combat: the tasks, lanes, waves, re-bless ledger, the cut order, G3′-real and the owner's decisions (decisions-log item 144). | Closed with S2 (decisions-log item 155, 2026-10-28); its rulings stand |
| `s3-plan.md` | S3, authoring: the tasks, lanes, waves, re-bless ledger, the cut order, G2 + P3, P1 FULL, P6 and the owner's decisions (drafted under decisions-log item 153, adopted under item 154). | Closed with S3 (decisions-log item 226, 2026-11-27); its rulings stand; the stage that follows S3 is S4 "Signals", whose first half is "the three towers" (decisions-log item 227) |
| `s4-plan.md` | S4 "Signals", its first half, "the three towers": the order of work (the external seat and the AI play harness, the defeat rule, three slices), what each step moves, the harness's counts, point 70's value, the weak points and fallbacks; the second half named, not planned (decisions-log item 229). | Current: S4's first half's plan |
| `external-seat.md` | The experimental external seat as it stands: one seat of a headless match played by a program outside the game through `gamectl match` (what the seat is, the commands, the bridge, exit codes, what a player that cannot see still lacks). Its decision is decisions-log item 231. |
| `holdfast.md` | The design of rotes and trust: rotes of cards, holdfast's five grades, surety, buried cable and network access control, the last Keybearer's defeat, the first round's match setting, and the direction for radio, each rule marked ruled, decided or open (decisions-log item 229). | Current: the design of record for S4; the later decision where the spec differs |
| `targeting.md` | How a playbook names where and what: names and descriptions, the three reading rules, "nearest", sites, the S1 slice (decisions-log item 127 (12)). | Current; the later decision where the spec differs |
| `radar-lab-research.md` | The radar lab's research memo: monostatic, Doppler, bistatic and passive, and solar-illuminated radar, with published validation cases, the lab's architecture and a phased build plan (decisions-log items 138 (4) and 140); its appendix A, a reference script kept as text, reproduces its computed values. | Current: the radar lab's plan of record; its remaining operator questions open |
| `handoff.md` | State of the design at the end of pass 6: what was decided, what was next, working conventions, the build plan in one line. | History as of pass 6; for the plan's shape now, see decisions-log item 227 |
| `6c-language-investigation.md` | The 36-agent investigation into the plan format and script language that produced the v1 simplification (JSONC now; TypeScript-family scripts run externally in v1.1; no embedded runtime). | Input to §2.7; roadmap material for v1.1 |
| `review-agenda-draft5.md` | The draft-5 review agenda: 30 items, the editorial fixes applied, and the moot and rejected lists. | Historical |
| `../spec/airgap-spec-v0.5.html` | Draft 5 of the spec, kept for diffing against draft 6. | Archive — do not implement from it |

## The short version

v1: everything that executes is Rust — the commander's **operator**, the beacons' **mandates**, the
units' and buildings' **programs**. Everything the player touches is data: **playbooks** and
templates as JSONC (canonical `gp.v1` proto JSON plus comments). No script runtime, no MCP, no SDK,
no published API, no AI seats, no manual control. The Seat Gateway is internal but agent-shaped;
script seams are reserved in the proto and never read. The sim is deterministic at 20 Hz on integer
maths, hashed every tick. Build plan: through S3 as the spec's weeks; after S3, S4 "Signals",
whose first half is "the three towers", then S5 "Commanders", S6 "Library", S7 "Watch and
share", hardening and v1, each detailed when the one before it closes and dated per stage, their
earlier contents carried as inputs; the fun gate moves to after the first new-core milestone
(decisions-log item 227). Versions are stage-based before 1.0, the minor number the stage: S3's
build is v0.3.0, the three towers' v0.4.0-m1 and S4's close v0.4.0, and the public preview is
whichever version passes the fun gate, the public v0.1 being retired (item 228). From S4 a
unit's or a structure's program is a rote of rule cards the player can author (the rote form is
the main session's design; the owner can veto), which are data like playbooks; the first build
is towers only (item 227, the owner's points 20, 28 and 36). Trust in a beacon's machines is its
holdfast, earned by the commander's hand, and beacons pass marks by buried cable before radio
arrives in S4's second half (item 229; `holdfast.md`, `s4-plan.md`).

Implementation rules — determinism, crate boundaries, contract files, security, commits, CI, the
definition of done — are in [`../../AGENTS.md`](../../AGENTS.md), not here. Contribution mechanics
are in [`../CONTRIBUTING.md`](../CONTRIBUTING.md).

## Changing a design document

These files are contract files (`AGENTS.md` §5). A design change is not a refactor: open a PR that
states what decision changed and stop. New decisions are logged in `decisions-log.md` as they are
made — continue §2.7 or open §2.8 for the next pass — and the spec is updated to match, never the
other way round.
