<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# S4 "Signals", first half: the three towers — plan

**Drafted under decisions-log item 229**, from the owner's answers recorded in items 227 (points
36 to 39), 228 and 229, and from the design in `holdfast.md`. It plans **the first half of S4
only**: the owner approved that each stage is detailed only when the one before it closes (item
227, point 37), so S4's second half is named here (section 8) and planned when the three towers
closes. The design it builds is `holdfast.md`; where this plan and that document disagree, the
design wins, and where either disagrees with the decisions log, the log wins (`README.md`,
"Precedence").

Every engineering call here is the main session's or this plan's, **decided; the owner can
veto**. Every count of pull requests and every "moves" below is an estimate of what the work will
touch, not a measurement; each pull request names what actually moved and why (AGENTS.md section
4.8 and section 5). No dates are given: dates are re-estimated per stage (item 227, point 37). The
reviews that shaped this plan are AI reviews, not independent review.

**Baseline.** `main` at `234ee0e6` (#261), tagged `v0.3.0`, S3's build (item 228; item 229 (9)).
At that commit `SNAPSHOT_VERSION` (`crates/sim/src/snapshot.rs`) is 16 and the save's
`SAVE_FORMAT_VERSION` (`crates/gateway/src/save.rs`) is 4.

## 1. Goal and end state

**The goal.** In one real match, a tower runs a rote the player can open and edit; the basic
tower shoots whatever crosses its motion sensor, the player's own drones included; a vouch by the
Keybearer's own hand unlocks a friend-or-foe card; a commission and buried cable unlock marking
contacts for other towers; the Ledger checks the cards; and the recap shows which card fired and
on what reading. This is item 227's point 36, "the three towers", with the licence for cards
replaced by holdfast and radio replaced by buried cable (item 229, points 56, 57, 61 to 65 and 69).

**What the owner plays at the close.** The owner's part is ruled (item 227, point 39): the
milestone closes when the owner has played the three-towers build and says whether writing a
tower's program is readable and fun. That verdict is the milestone's gate. The close is a
milestone review inside S4, not a stage demo: the merge delegation runs through both halves of S4
to S4's stage demo, and the owner can revoke it at the three towers' close, as at any time (item
228, point 42).

**The milestone's tag** is `v0.4.0-m1` (item 228, point 41; AGENTS.md section 9, "Versions and
tags"). It goes on the merge commit of a pull request that does nothing but set `VERSION` to
`0.4.0-m1`, after the owner's word for that tag.

**The version on `main`.** Between tags, `main` carries `0.4.0-dev` throughout S4, over a
`## 0.4.0-dev` changelog section that collects what lands (AGENTS.md section 9, "Versions and
tags"); its own pull request (#262) sets it, and it merges before any new work.

**Machine-checkable at the close** (AGENTS.md section 10, as it applies to a milestone; the stage
close at S4's demo applies all of it): `cargo xtask ci` green on the matrix; every moved chain and
golden explained in the pull request that moved it; the three towers' playbooks and rotes
round-trip, verify and render identically; scenarios with event and hash assertions for each
slice; the nightly bands recorded, not chased (item 227, point 35); and the AI play harness's
counts of section 7 recorded for each slice.

## 2. Order of work

Six steps, in this order. Each later step takes `main` as the one before it left it.

| Step | What | Why first |
|---|---|---|
| 0 | The external seat and the AI play harness | "Yes, build it first (Recommended)" (item 227, point 38); approved as the staged external seat (item 229, point 44) |
| 1 | The defeat rule | It changes how a match ends and moves every recorded match, so it lands alone, early (item 229, point 68) |
| 2 | Slice 1: the Sentry and raw cards | Everything later reads rotes |
| 3 | Slice 2: the vouch and Challenge | Needs rotes; brings the first match setting |
| 4 | Slice 3: buried cable and Mark | Needs Vouched |
| 5 | The milestone's version pull request and the owner's play | Item 227, point 39 |

**Contract pull requests inside a slice**, in merge order (decided): the proto and the rules table
first (`proto/**`, `rules/rules.v1.json`); then the sim; then the verifier and plan-core; then the
gateway; then the operator and `library/`; then the client (`crates/client-gdext`, `godot/`). A
slice may be one vertical lane whose review count is set by its riskiest file (item 227, point
30); it merges in that order either way, and two lanes share a crate only when their briefs name
disjoint files (AGENTS.md section 6).

**Review class.** Every step below touches `crates/sim/**`, `proto/**`, `rules/**`, a golden's
content or a section 5 contract path, so every one gets **two reviews** with different lenses
(AGENTS.md section 6, "Reviews by risk"). Each pull request's body states its count and the
riskiest path that set it.

### Step 0: the external seat and the AI play harness

**What it gives.** A local program (the main session's AI first) can take a seat headless with
exactly a seat's rights, fog-filtered, rate-limited and logged, and play many fast matches against
Easy; a headless match runs about 10 times real time (item 229, point 44). Its results are
recorded, not used to tune (item 227, point 35).

**Its own plan.** It is its own contract pull request, with its own brief. It carries the rule
changes that points 44 and 47 approve: AGENTS.md sections 2, 7 and 11 and CLAUDE.md "Permissions",
including the definition of "a real seat" (item 229 (1)). This plan does not design it; it only
needs the harness to report the counts of section 7 for each slice.

### Step 1: the defeat rule

**What the player sees when it merges.** A seat whose last Keybearer is eliminated is defeated;
with one Keybearer per seat, that is its Keybearer's death. Nobody comes back. A seat with no
Stakes left is still defeated (item 229, point 68; the second rule is the main session's
reading).

**Crates.** `proto` (the playbook's required `on_death` block, `gp.v1` field 4, and its
`on_respawn` options, which go; fields are never renumbered or reused, so they are reserved, and
the pull request states how Load answers a file that still names them), `sim` (the respawn, its
delay and the death counter go; the defeat condition), `verifier` and `plan-core` (the block's
checks and its prose), `gateway` (the recap and the match's end), `operator` and `library/` (every
library playbook carries the block), and the client and `godot/` (its fixtures and the new
playbook template carry the block).

**Contract surface.** The event vocabulary: `commander_respawned` (`EventKind`, id 11) is retired,
and its id is never reused (AGENTS.md section 5). AGENTS.md section 11's v1 vocabulary names
`on_death`, so that line changes with this pull request. The spec's section 4 "Death" rule is
superseded by item 229 and is brought in line here.

**What moves (expected).** Every scenario chain in which a commander dies, and every match whose
end it changes; the golden playbooks and every verifier report golden that holds an `on_death`
block; the snapshot version and the save format if the death counter or the pending respawn
leaves the snapshot. Each chain that moves is named, with this rule as the reason.

**The harness counts** before it is closed: commander deaths to the seat's own fire (zero is
expected while point 70's value is "does not fire", section 3).

### Step 2: slice 1, the Sentry and raw cards

**What the player can do when it merges.** Place a Sentry facing north, east, south or west; open
its rote (`sentry.basic`, one card) in the editor's rule list, edit or reorder the cards and have
the commander install it at the Stake by hand; see the arc on the map in the editor and in the
Push; turn a standing Sentry by an interface row. A raw Sentry fires on anything that moves in its
arc, the seat's own drones and units included. The recap names the Sentry, the card and the
reading; a Ledger memo explains the first friendly fire. Every Stake behaves as Unkeyed
(`holdfast.md`, sections 12 and 17).

**Crates.** `proto` (the rote and card messages and the install row in `gp.v1`; the Sentry's rows
in the rules table; the events and the view feed's Sentry in `gp.api.v1`), `rules/`, `sim` (the
Sentry, its seat-blind fire, the card evaluator, installed rotes as match state, the new events),
`verifier` and `plan-core` (the cards' checks, a card's size counted honestly, the arc warnings
with their Fix, the rotes' JSONC round-trip and prose), `gateway` (the wire, the recap lines),
`operator` and `library/` (`library/rotes/` with `sentry.basic`; Easy places Sentries facing away
from its own holdings, recorded and not tuned), `bench` (a card-heavy case for the verifier's
timing gate), and the client (the rule list's rote editing and the arc).

**What moves (expected).** The snapshot version and the save format (installed rotes are hashed,
saved state; AGENTS.md section 4.8); every verifier report golden (the rules table gains rows, the
snapshot gains facts); the event vocabulary, additively. Chains move where a Sentry stands; new
tables are hashed as blocks after their rows (item 136), so a match with no Sentry keeps its chain.
Once Easy places Sentries, every scenario with a built-in seat and the nightly result move.

**Gates it must hold.** The verifier's P1 (QUICK at most 5 ms p99 and FULL at most 50 ms p99 at the
playbook size budget) with the card-heavy case added, and G3′-real's alarm (`cargo xtask
g3-budget`) with Sentries standing (AGENTS.md section 9, item 11).

### Step 3: slice 2, the vouch and Challenge

**What the player can do when it merges.** Have the Keybearer vouch for an entered Stake by hand;
install `sentry.iff` ("when a contact answers, hold fire; when something moves in the arc and has
not answered, fire"); read each Stake's holdfast mark on the map and each rote's behaviour at each
grade in the editor; see a skipped card and its reason during the Push and in the recap; choose
the first round's match setting in the lobby (the newcomers' Probation match plays Home vouched,
a standard match Cold start; item 229, point 58). The Ledger refuses what can never work, warns
about what may not, and offers Fixes (`holdfast.md`, section 13). A player's rote can be saved to
the local library and loaded in a later match.

**Crates.** `proto` (the vouch press row, the match setting, the new events and failure reasons,
the grade in the view feed), `rules/`, `sim` (the pressed record, the Ledger's entry at the
settlement after the licence check, the vouch, Challenge, the skipped-card rule), `verifier` and
`plan-core` (the refusals, warnings and Fixes, by the same shared function the sim uses for
grades), `gateway` (the setting in `match.json`, the config line and the save; the fog rule that
an enemy sees only a Stake's grade in force), `operator` and `library/` (`sentry.iff`; whether
Easy vouches), and the client (the holdfast mark, the setting in the lobby, the editor's
per-grade view).

**Contract surface.** The match setting is the first lobby option that changes the match itself,
so one contract pull request names every format it reaches: `match.json`, the config line in
`user://last_match.txt`, the save container and the event vocabulary (AGENTS.md section 5).

**What moves (expected).** Every committed match's chain, since every match reaches a settlement
and enters its Stakes (`holdfast.md`, section 17); the snapshot version and the save format; every
verifier report golden; once Easy vouches, the nightly result and every scenario with a built-in
seat.

### Step 4: slice 3, buried cable and Mark

**What the player can do when it merges.** Order build drones at a Build Stake to lay buried cable
between two Commissioned Stakes inside its holding; commission a Stake vouched by hand, putting it
on the seat's roll; install `sentry.mark`, which posts unanswered contacts as marks and may fire
on a marked contact inside its gun's range and outside its own arc; see a cut reported once with
where it happened, a quarantined Stake work as Vouched, and drones re-lay a cut length when the
writ is set to rebuild (`holdfast.md`, section 8). Two or more cards may act on one beat, so "the
first true card wins each of fire and signal" arrives here.

**Crates.** `proto` (the cable build target, which needs a second end beside a Build target's one
anchor; the commission press; the cable's price and depth rows; the material table and the cable
events in `gp.api.v1`), `rules/`, `sim` (the cable target and material, the digs that skip a seat's
own cable, the roll, the joins and quarantine once a tick, Mark, the once-only cut report), the
verifier and plan-core (the cable refusals and warnings), `gateway` (the material table, the
events), and the client (the material's palette row, the seat's own cables on its map, Mark).

**What moves (expected).** The snapshot version and the save format (cable rows and the stored
once-only bit); the material table and its client pin; every verifier report golden; chains where
cable is laid or a Stake is commissioned; and, if Easy commissions, the nightly result and every
scenario with a built-in seat.

### Step 5: the milestone's close

The pull request that sets `VERSION` to `0.4.0-m1` (two reviews, since it touches `xtask/**`);
the tag, pushed by the main session on the owner's word; the build the owner plays; and the
owner's verdict (section 1).

## 3. Point 70's value

The owner deferred whether a raw Sentry fires on its own seat's Keybearer (item 229, point 70).
The build keeps the choice as **one rules-table value**, read by the sim's Sentry fire, so either
answer is a change of data, not of code. **The first build carries "does not fire"**: the main
session's choice, told to the owner and not asked (item 229, point 70's record; the owner can
change it), so that a test match is not lost to one's own tower. Its row lands in slice 1's proto
and rules pull request, and its source carries the marker
`PLACEHOLDER: whether a raw Sentry fires on its own seat's Keybearer (item 229, point 70) — owner, at the three towers' close`.
A raw Sentry fires on its own seat's drones and units whatever the value.

## 4. What each slice moves, in one table

| Step | Snapshot and save | Verifier report goldens | Chains | Contract formats |
|---|---|---|---|---|
| 1, defeat | if the death counter leaves the snapshot | those with an `on_death` block | every chain where a commander dies | `gp.v1` (`on_death`), the event vocabulary |
| 2, slice 1 | yes (installed rotes) | every one | where a Sentry stands; with Easy's Sentries, built-in scenarios and the nightly | `gp.v1`, `gp.api.v1`, the rules table, the event vocabulary |
| 3, slice 2 | yes (pressed records, the entered mark, the setting) | every one | every committed match | the setting in `match.json`, the config line and the save; the event vocabulary |
| 4, slice 3 | yes (cables) | every one | where cable is laid or a Stake commissioned | `gp.v1` (the cable target), the material table, the event vocabulary |

All entries are expected, not measured.

## 5. Easy in the three towers

Easy uses the same rows as a player and only the shipped rotes, places Sentries facing away from
its own holdings, and is recorded, not tuned, while balance is paused (item 227, point 35;
`holdfast.md`, section 15). Whether Easy vouches in slice 2 and commissions in slice 3 is that
slice's lane's to decide and to state, since it decides which chains move.

## 6. What the owner is asked, and when

Nothing in this plan needs the owner before step 0. The owner is asked: the tag `v0.4.0-m1`, at
step 5; the verdict on writing a tower's program, at the close; and the questions `holdfast.md`
section 20 holds, each when the step that needs it arrives (point 70's value before slice 1
merges only if the owner wants to change it). A slice that finds a question outside the design
holds that piece and batches it (CLAUDE.md, "What is asked and what is told").

## 7. What the AI play harness measures before each slice closes

Taken from the design's critics and its weak points (`holdfast.md`, section 19). Each count is per
seat, recorded and not tuned on (item 227, point 35).

- **Step 1:** commander deaths to the seat's own fire.
- **Slice 1:** Sentries built per seat; friendly kills by cause (repair, pursuit, mining,
  commander), per seat and per difficulty; how often a newcomer's first Sentry kills the seat's
  own drones; in three-seat matches, kill credit earned by Sentries against seats not targeting
  their owner.
- **Slice 2:** the share of Sentries that ever run a Challenge card; walk-seconds spent on vouch
  presses; the win rate of a "never vouch" arm; a dominant rote, looked for before the slice
  closes; rounds re-driven Stakes spend Unkeyed.
- **Slice 3:** cable voxels laid, with the `$` and drone-seconds they took; cuts by cause (enemy
  aimed, enemy stray, own mortar, own dig, collapse); time from a cut to its re-lay; Stake-ticks
  with Commissioned pressed but out of force; Stakes that could not be cabled; towers silent while
  an enemy is in their arc, after a fall; friendly kills in the 30 s after a fall; the share of
  Stakes commissioned; the win rate of a "never commission" arm.

## 8. S4's second half, named and not planned

Planned when the three towers closes (item 227, point 37). It is named to hold:

- **surety**, with its price (item 229, points 57 and 69; `holdfast.md`, section 7);
- **every gun onto cards**, the autocannon and the mortar losing free identification (item 229,
  point 51);
- **each machine sensing for itself**, a seat's machines no longer sharing one perfect map
  (points 51 and 62);
- **radio with the Signal spar and the security phase**: Lapsed, tamper and Outcast, when
  emissions matter (points 61 to 63; `holdfast.md`, section 11), with the over-the-air attacks
  that move into S4 (item 227, point 37).

**Carried from S3's close and still held** (items 226, 227 and 228, as they record them):

- S4's earlier contents, carried inputs re-placed when the second half is detailed: the symmetric
  three-way generator, the award fund and P7 (item 227, point 37); the licence triggers of the
  Radio Mast and the Sensor Spire, which the spec gives to S4.
- Rule A for a beacon's "nearest", adopted and to be built with fog (item 226 (5)); and
  `targeting.md`'s octile sentence, which that lane rewrites (item 228 (6)).
- The scouts' routing, folded into the new sensing (item 227, point 32); the radar lab, folded into
  radio warfare (item 226 (6), point 18).
- Exploits and beacon capture, placed when S4 closes (item 227, point 37).
- The merge queue and the test tiers' CI, their own pull request after the repository moves
  (item 227 (7) (a)); the dependency-update lane, which goes first of the post-S3 lanes (item 227,
  point 32).
- The re-dating of the "owner, at S3's demo" markers and of the three "the v0.1 gate" markers, told
  items for the next pull request that touches those files (items 227 (6) and 228 (6)).
- Held for the owner: the owner's global release rules (point 26); the decision tick's kinds'
  disagreement; item 217 (6)'s veto list and its two screenshot observations; the lane limit's
  wording in spec section 15; `vcap`'s two questions and PR #183's wire gaps; item 162 (8)'s
  stage-forecast proposal; the release branch (item 228 (9) (g)); and the questions item 229 (12)
  adds.

## 9. Known weak points and fallbacks, written before the stage starts

The weak points are `holdfast.md`'s (sections 18 and 19). The fallbacks are this plan's (decided;
the owner can veto), written now as AGENTS.md section 10 item 6 asks; taking one is the owner's,
at the milestone review.

1. **Holdfast governs only the Sentry until S4's second half**, so "never vouch" may win the
   milestone's matches. Accepted by the owner (item 229, point 51). The milestone proves the
   mechanics, not that trust decides matches; the harness's "never vouch" and "never commission"
   arms record how far.
2. **Round 1 could be nothing but friendly fire.** The arc drawing, the Ledger's warning with its
   Fix, and the memo answer it; the harness counts it in slice 1.
3. **One rote may dominate**, making the cards an upgrade path. The reply delay is the first
   counterweight; the harness looks for it in slice 2.
4. **Buried cable has almost no counterplay** (accepted, item 229, point 65).
5. **The arc warnings are rough**: straight-line geometry between named places.

**Fallbacks.**

- **F1, scope.** If slice 3 is not playable when slices 1 and 2 are, the three towers may close on
  slices 1 and 2, with cable and Mark moved to the head of S4's second half. The owner decides at
  the review.
- **F2, the verdict.** If the owner finds writing a tower's rote not readable or not fun, the
  milestone does not close: the next step is a short design batch on the cards and the editor,
  brought to the owner before more slices are built.
- **F3, timing.** If the card-heavy case breaks P1, or Sentries break G3′-real's alarm, the slice
  does not merge: the lane brings the measured figures, with the cost model's options, to the
  owner. No cap on Sentries or cards is added for balance (AGENTS.md section 12).

## 10. Deliberately left out of the first half

Rotes for units and for today's guns; commander styles, the roster, Sworn and Bound (S5); surety;
radio, the Signal spar, Lapsed, tamper, Outcast and forged marks; line of sight beyond a
challenge's; the code view (S6). Everything in AGENTS.md section 11 stays out.
