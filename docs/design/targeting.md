<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Targeting: how a playbook names where and what

**Adopted 2026-09-29 (decisions-log §2.7 item 127 (12)).** The owner asked for several rounds of
investigation, then said to "perform another pass of investigation. then adopt your recommendation".
Where this page and a §2.7 item disagree, the item wins, except where this page amends the item (the
amendments are listed at the end). Where this page and the spec disagree, this page is the later
decision and the spec is updated to match.

## Why

At the walking skeleton's demo a carried playbook placed a beacon at the same voxel every round
(item 126 (3), F4). The owner said a positional playbook "only really makes sense for the first
round": re-authoring one every round "wouldnt be fun imo but could be done, especially with the help
of AI design", while "the same positional plan round after round doesnt make sense as a strategy".
Asked what a new Lull should open with, the owner said: "consider further. the most obvious strategy
i can think of is build things based on the location of things that are already visible". The
vocabulary had to be forward-looking, because it "will impact much of the scripting and ai and
ai-assist to come" and "is hard to undo once playbook files use it". It had to be "something elegant
since this is the default/simple built-in". And "nearest" "does not need to be optimized but at the
same time cannot be stupid or overly simplistic".

The same vocabulary has to serve a first-time player in the editor, the built-in operator, v1.1's
external scripts, v1.2's live agents and the advisor, without becoming a language (AGENTS.md §2 and
§11). The design came out of:

- three research passes: the scripting and AI future, prior art in programmable strategy games and
  LLM game agents, and the codebase's own binding times;
- a panel of three independent designers (elegance, future authors, rigour);
- two adversarial critics (the player and scope; contracts, determinism and fog);
- a final pass that walked ten scenarios through it, and a separate investigation of "nearest";
- two reviews of this page.

## The model

A target is a **name** or a **description**.

- A **name** is a fixed id:
  - `b_NN` for one of the seat's own beacons (per seat from S1);
  - `e_NN` for another seat's beacon, a per-viewer handle minted on first sighting, as the view feed
    already does for units and structures;
  - `vent_<x>_<y>` for a vent and `seam_<x>_<y>` for a seam. The view feed's `s_` and `u_` prefixes
    are taken.
- A **description** is a pick from one closed catalogue per kind: a rank and a few flat filters, for
  example "the nearest vent not covered by my spheres". No variables, no arithmetic, no offsets, no
  loops, one level deep. Tags stay the only author-chosen names.

The sealed file always holds the text as the author wrote it, never what it resolved to. A carried
playbook therefore reads its descriptions again every round, which is what makes carrying it
sensible. Item 28's carry rule stands.

## Three reading rules

1. **A step reads all its targets when it starts**, including the ones it writes into a new beacon's
   initial settings or an interface row, and keeps them until it ends. A step checks its bound
   features at each decision. A lost one fails the step, and no row ever commits a lost target.
2. **A beacon keeps the target it was given until you change it on site.** A description a beacon
   holds reads again only when its target is lost (a vent destroyed, a seam spent); a name idles
   when its target is lost.
3. **A condition checks now**, at every evaluation.

Named exceptions, unchanged:

- Item 13's Attack target, pinned at seal (S2).
- The reflex's `safest`, read when it fires.
- The fallback's posture target (any `BeaconRef` or `Location`: hold, shadow, patrol), read again
  every decision until the fallback latch (deferred).
- A mandate's own `seam_choice`: a choice inside the Mine program, held until the seam is spent,
  using the same "nearest". A Mine beacon may dig a seam other than the one it was placed to cover.

A held target fails only when it is **lost**. Filters are not re-checked while a target is held,
because the holder's own act (covering a vent, building on it, mining a seam dry) must not
disqualify its own target.

## Failure

Step failures keep their wire ids (`crates/sim/src/interpreter/state.rs`: additive only, never
reuse, never renumber). Each is handled by `on_fail`. A condition over an empty reference is false.
Nothing substitutes silently.

| Case | Code |
|---|---|
| A description matches nothing, or a name is hidden, absent or someone else's | `no_target` (2), one answer for all, so a guess reveals nothing |
| A walk cannot arrive (sealed in, unreachable) | `no_path` (3) |
| The commander leaves the visit's range mid-visit | `out_of_range` (4), unchanged |
| A held beacon died | `beacon_gone` (5), unchanged |
| Candidates match but none has a legal site; a site became illegal | `illegal_site` (6) |
| A held feature is lost (vent destroyed, seam spent) | `feature_lost`, a new id (11) |
| `not_own`, which told a seat whether an unseen enemy beacon is alive | retired: id 10 is never reused, and `no_target` answers instead |

## Names

- **Vents and seams** are named by their generation anchor column, the patch or disc centre, with no
  z (craters change z): `vent_120_88`. Players see "Heat vent (120, 88) · rich".
  - An anchor id reveals no count and no order, so it stays safe under S3's fog. A guessed id
    answers `no_target` exactly as a hidden one does.
  - S1 adds a map-generator check that refuses a duplicate anchor or overlapping footprints with a
    `MapError`. Today a zone's vent and seam are kept apart only by the committed distance bands.
  - The feature table is a pure function of the seed, the rules and the occupied seats (item 38 lays
    no features in an empty zone). It is regenerated on restore and not hashed per tick. A new
    mapgen features golden pins the anchors, which is a golden-format change.
  - A feature's liveness is derived by scanning its footprint when it is read (9 voxels for a vent,
    at most about 150 for a seam), so it adds no hashed column and needs no hook in `set` or
    `crater`. A vent is lost when no exposed vent material lies in its footprint; a seam when no ore
    does.
  - `power.rs`'s `one_vent` moves onto the table.
- **Own beacons** become per seat (item 127 (13)). A seat's core is always `b_00`, so a template can
  name it anywhere. A `b_NN` resolves only among the seat's own beacons.
- **Other seats' beacons** are `e_NN`, per viewer, in first-sighting order. `get_beacon`,
  `list_beacons`, the verifier's `Scope` and events use them, and a seat never sees another seat's
  `b_NN`.
- **Table room** becomes per seat so that a full table stops leaking the others' counts: each seat
  gets the world total divided by the seat count, rounded down (item 63's 40 beacons and 300 units
  give 13 and 100 at three seats, 20 and 150 at two). Build targets and structures follow. This
  amends item 63; the split is S1's to tune.

## Descriptions (S1)

`FeatureRef { oneof ref { string feature_id = 1; VentPick vent = 2; SeamPick seam = 3; Covered covered = 4; } }`

- `VentPick` and `SeamPick` each carry a `rank` (S1: `NEAREST`) and a `coverage` filter (`ANY` or
  `UNCOVERED`).
  - `RANK_UNSPECIFIED = 0` is a verifier error.
  - Under `covering`, `COVERAGE_UNSPECIFIED = 0` is a verifier error.
  - Under `on`, `coverage` must be omitted.
- **UNCOVERED** means outside every sphere of the seat's own living beacons, awake or dormant, since
  a dormant beacon keeps its sphere. It never reads another seat's state.
- `BeaconRef` stays as it is. The picks are per kind, and "one catalogue" means one rank vocabulary
  and one resolver, not one message.
- A description ranks from the seat's own state and the static features.
  - The seat order of the decision phase cannot change a result.
  - Feature liveness and reachability read the live world, so an unseen enemy mining out a contested
    seam, or cratering a path, changes a seat's answer. This is accepted for S1, as item 108 (1)
    accepted unseen voxel edits. S3 bases both on the seat's terrain knowledge.

### Nearest

**Nearest = the least estimated travel from the origin to the feature, over the terrain the seat
knows. Unreachable candidates are skipped. Ties go to the lowest anchor y, then x.**

- **The metric** is the item-61 estimator's integer `cost`, from which the editor's ETA is derived.
  For one walker the two are monotone, so what a chip says and what the sim picks agree. It prices
  climbs, pits and craters, and never picks a target the commander cannot reach.
- **The origin:**
  - `covering`: the commander's column when the step starts.
  - `on`: the target beacon's anchor, or, for an initial row, the site `covering` chose.
  - A description a beacon holds: that beacon's anchor.
- **It measures to** the feature's anchor column. When that column cannot be stood on or is not
  connected, it measures to the first standable connected footprint column in (y, x) order.
- **Unreachable:** a feature with no connected footprint column is not a candidate. When nothing
  remains, the answer is `no_target`, and the recap says so ("3 matched, none reachable").
- **Ties:** `(cost, anchor y, anchor x)`, the pathfinder's own node order. Integers only; the order
  is total.
- **Cost:** at most one estimate per candidate (six or fewer per kind on an S1 map), pruned by the
  octile lower bound, and only when something reads. S1 measures it against the tick budget.
- **Where it runs:** in the sim (the tick) and in the gateway (`resolve_refs`, `estimate_route`),
  which lend the host's pathing graph. The verifier does not rank (see Surfaces).
- **Not in conditions in S1.** A condition reads every 250 ms, and re-ranking every evaluation
  thrashes. Feature `NEAREST` there is refused; the arm stays reserved.

This rule was adopted under the design answer's "adopt your recommendation", after the investigation
the owner asked for. It is in the owner's one-screen summary for overrule (item 127 (11)).

## Sites

`Location` gains two arms, `FeatureRef on = 10;` and `FeatureRef covering = 11;`, and its reserved
range narrows to 12–49.

- **`covering`**, legal only in `PlaceBeaconStep.at`.
  - The sim ranks the candidates and takes the first one it can cover.
  - Around that feature it walks a fixed spiral: squared xy distance from the anchor, then squared
    xy distance from the commander's column at step start, then y, then x. The spiral is bounded by
    the sphere radius and excludes every feature footprint.
  - It takes the first column that is a legal site (inside one of the seat's own spheres, no
    stacking below), standable, reachable by the commander, and whose sphere holds both the
    feature's `on` column and its anchor point (the anchor column's standing point). The second
    test keeps a `covering` from binding a site where its own `covered {}`, which tests the anchor
    point first, would answer `no_target`; where the anchor column is free the two points are one
    (`fog` #90, decisions-log item 135).
  - The operator's private `site_for`, which walks out from the nearest own beacon along the line to
    the feature, is replaced by this rule in the sim (deleted by `oper` #96, decisions-log item 136).
- **`on`**, legal only in `BuildTarget.anchor` (in `add_build_target`, a Build mandate's settings
  and `initial`).
  - The pick ranks only vents whose `on` column lies inside the target beacon's sphere (for an
    initial row, the chosen site's sphere), measured from that beacon.
  - A vent is a candidate only if no live Generator of any seat and no Build target of this seat
    stands on its footprint, not counting the beacon being written to. A structure inside the target
    beacon's sphere is inside the seat's own vision, so this reads known state. It keeps a seat from
    buying a second Generator on a vent, whose second tap supplies 0 kW (`power.rs` `supply_of`).
  - The `on` column is the anchor column if it is free, otherwise the next free footprint column in
    (y, x) order.
  - A named or `covered {}` vent whose anchor column's standing point lies outside the target
    beacon's sphere is `no_target`, tested before any structure, so no answer depends on a structure
    the seat cannot see (decisions-log item 133 (3) (c), item 134). Built by `fog` #90 (item 135)
    as the sim's `anchor_in_sphere`, which the gateway's preview reaches through `on_vent_counted`.
    One residual is S3's fog work (the register's S3-20): an unplaced beacon's target sphere may
    reach past every sphere the seat holds, and inside it the structure tests read the whole
    footprint.
- **One structure per voxel, across seats.** A queued Build target is a per-seat claim, so another
  seat's unbuilt target stays hidden. Construction is refused where any live structure already
  stands, which is `illegal_site`. The first seat to build wins, the other's target fails, and the
  vent's heat stays one Generator's (spec section 5). This is taken on the recommendation, open to
  the owner's overrule.
- **`covered {}`**, legal only under `on` in a Build target inside the `initial` of a `place_beacon`
  whose `at` is a `covering` arm: "the feature this beacon was placed to cover". It binds as a name
  at deploy and idles like one when its feature is lost.
- **Build targets made through `on` are keyed by feature id.** `RemoveBuildTargetRow` accepts `on
  {feature_id}` only, and carry-over replaces by feature id.
- **No claim column in S1.** One commander per seat means one step reads targets per seat per
  decision. Beacons re-read held descriptions only when a target is lost. Vents cannot be lost
  before S2's craters, and a spent seam's re-read needs no exclusivity, since two Mine beacons may
  share a seam. S2 binds re-reads in ascending beacon id, with the per-seat target table as the
  claim.

## Companion changes

- **Walk-in.** `place_beacon` and `interface` walk to their target first.
  - The step's `timeout_ms` bounds the walk up to arrival. A site the commander cannot reach fails
    `no_path`.
  - The editor and `estimate_route` draw the implied leg.
  - Today such a step fails `out_of_range` or `illegal_site` when the commander is not already
    there; `skip_if` now expresses "only if already there".
  - `commander.placement_range_voxels` loses its role, which amends item 11. The rules row stays in
    the schema, documented as superseded, as item 90 did for others. In practice the placement range
    is already moot, because the deploy requires the interface range.
- **Restart keeps the placed beacon.** A step that death or the reflex clears after its deploy
  resumes as a visit to the beacon it placed, with the rows it had left, and reads nothing again, so
  it never pays for a second beacon. Handlers do not fire during a deploy or a commit.
- **No stacking.** A site on the same column as one of the seat's own live beacons is illegal
  (`illegal_site`). A minimum spacing between beacons is a tuning row S1 proposes, as a PLACEHOLDER
  for the owner. This also cures carried fixed-voxel files.
- **Bugs fixed with it.**
  - `estimate_route` sends every ranked selector leg (`nearest`, `weakest`, `most_threatened`) to
    the seat's first beacon (`crates/gateway/src/surface/knowledge.rs`).
  - `crates/sim/src/interpreter/cond.rs`'s module doc says selectors resolve once per step, though a
    condition resolves them at every evaluation.

## Surfaces

- **Ranking** happens where the pathing graph is: in the sim for the tick, and in the gateway for
  previews.
  - The gateway's `resolve_refs` answers over the frozen world. It is internal in S1, on
    `ADVISOR_METHODS`, and published with v1.1. Whenever the phase is not a Lull (a Push, a recap,
    the lobby, an ended match) it answers `PHASE_CLOSED`: the dispatcher's planning door closes it
    as a `plan`-scoped method, as it has since T9, and from `econ` #92 its handler holds its own
    gate in a Push as well, as defence in depth (decisions-log item 135 (3), which corrects item
    133 (3) (f)).
  - The editor cannot reach the sim (`CLIENT_WALL`) and the operator's library depends on proto
    alone, so both ask the gateway.
- **The verifier** checks vocabulary, legality of placement (which arm where), filters and
  existence, never rank. `report_hash` stays a pure function of `Scope`.
  - `Scope` gains the features (id, kind, grade, a live bit) and the commander's position.
  - A verifier that one day ranks carries the gateway's precomputed costs in `Scope`, hashed.
- **`get_map_summary.features`** (S1-48) lists every feature with its id, kind, grade, coverage by
  the seat and travel.
- **`estimate_route`** accepts `covering` and returns the site, so Easy needs one call per candidate
  and keeps no second site algorithm. The operator derives no ids itself.
- **The chip:** "now: Heat vent (120, 88) · 14 s · next (150, 20) · 16 s — nearest by travel from
  the commander, read when the step starts".
- **Lints:**
  - A description that matches nothing now.
  - "An earlier step places a beacon; this may read differently": a syntactic check that an earlier
    route step or any handler body contains a `place_beacon`, never a projection.
  - "Heat vent (120, 88) is already covered by your b_07" on a carried fixed site, with a Fix that
    swaps in the description.
- **The editor:** a click on a vent offers "Place beacon covering this vent" (a name); Alt-click
  turns it into "the nearest vent you can cover". Under item 28 the editor renders every S1 arm,
  `on` and `covered {}` included, or the verifier refuses what it cannot render.
- **Templates** are rewritten with descriptions and lose their positional wizard parameters; Easy
  emits names.
- **Messages.** On re-seal the Lull says what this round will do ("this round: new beacon near (150,
  20), $ 60", or "no vent you can cover"). The recap names why a step found nothing.

## What the map means for it (a finding, not a rule)

On the golden seed, seat 0's start vent is covered in round 1. The nearest uncovered vents after
that are contested ones, about 80 voxels away by the final pass's reading of the map. That is beyond
a single beacon's 48-voxel reach, so a carried "cover the nearest vent" finds nothing to cover in
rounds 2 and 3. That outcome is deterministic, harmless and honest, and the Lull says so.

Reaching those vents takes stepping-stone expansions. The skeleton's operator made them with its
private `site_for` ("one expansion out on the line towards it"), which `oper` #96 deleted with the
site heuristics, so Easy, like a human seat, now reaches only the features a site covers. A `toward`
site arm (`Location` 12) would make them; S1's plan, decision 3 (decisions-log item 128), deferred it
to S3, and Easy's expansion past its reachable vents waits for it (decisions-log item 136 (3) (q)).

## The S1 slice

The owner chose the minimal slice (item 127 (12)). As amended by the final pass and the reviews, S1
builds:

- **Proto and data:**
  - the feature table and its mapgen check;
  - the `vent_`/`seam_` names, per-seat `b_NN`, per-viewer `e_NN` and per-seat table room;
  - `FeatureRef` with `VentPick` and `SeamPick` (`NEAREST`; `ANY` or `UNCOVERED`), `covered {}`;
  - `Location.on` and `Location.covering`.
- **Behaviour:**
  - the three reading rules and the failure table;
  - the "nearest" rule, `covering`'s spiral and the `on` rules;
  - one structure per voxel;
  - walk-in, restart-keeps-beacon and no stacking.
- **Surfaces:**
  - `Scope`'s features;
  - `resolve_refs` (internal) and `get_map_summary.features`;
  - `estimate_route` accepting `covering`, and its selector bug fixed;
  - the three lints;
  - the chip;
  - the editor's click and Alt-click;
  - the rewritten templates, Easy's names, and the Lull and recap messages.

The owner chose it at about 10 to 12 agent-days. The final pass's and the reviews' additions
(walk-in's timeout and `no_path`, restart, per-seat room, `e_NN`, one structure per voxel,
`estimate_route` accepting `covering`, `resolve_refs` on `ADVISOR_METHODS`, travel instead of octile
distance) put it nearer 14 to 16. S1's plan confirms the figure.

## Deferred, with their numbers reserved

- `RICHEST`, grade filters, `in_reach` and enemy-known filters (S3).
- Feature `NEAREST` in conditions, a risk-weighted rank, a `from` origin override.
- The non-hashed `target_bound` event for replay readers.
- The fallback latch.
- Publishing `resolve_refs`, `extract_template` and rebind (v1.1).
- Aligning `BeaconRef.nearest`, which is octile distance today, with this page's "nearest", before
  the public v0.1.
- Withholding the seed from seat tokens (S3, item 127 (13)). A local agent with file access can
  still read `match.json`, and the docs say so.
- Basing liveness, reachability and the estimator's connectivity oracle on the seat's terrain
  knowledge (S3).

## Contract pull requests (S1)

1. **Proto:**
   - `proto/**` and a scoped `buf.yaml` ignore for the narrowed reserved range;
   - the generated tree, and the `get_schema` and docs goldens;
   - every proto comment the change makes wrong: `Location`'s "one selector catalogue rather than
     two", `PlaceBeaconStep.at` and `Step.place_beacon` (the placement range), `Step.interface`,
     `BuildTarget.anchor`, `RemoveBuildTargetRow`, `BeaconRef.beacon_id` (bound ids), and
     `MineSettings`' PLACEHOLDER.
2. **Determinism:**
   - `b_NN` per seat, `e_NN` per viewer, per-seat table room;
   - a snapshot and save version bump that refuses old saves, with old private replays checked by
     `match.json`'s `gateway_version` (bump `GATEWAY_VERSION`);
   - the new hashed state: a binding per description in a step's `PlanState`, each Build target's
     description and bound feature id, the seat's beacon ordinal, and the restarted step's placed
     beacon and remaining rows;
   - every chain.
3. **Behaviour:** walk-in, restart, no stacking, one structure per voxel, the resolver and the
   sites. These move the chains too, so 2 and 3 land together and re-bless once.
4. **Goldens and docs:**
   - the verifier's `report_hash` goldens and the diagnostic catalogue (new codes);
   - the new mapgen features golden (a format change);
   - the spec and the amendments below;
   - AGENTS.md: §3 rule 2's wording on previews, and a §5 sentence that a reserved range may be
     discharged by the thing it was held for.

## Irreversible once playbook files use them

- **Names:** the spellings (`b_NN` per seat, `e_NN` per viewer, `vent_<x>_<y>`, `seam_<x>_<y>`).
- **Proto:** the field numbers, JSON names and enum value names above.
- **"Nearest":** its metric, origin, fallback column order and tie-break chain.
- **Reading and failure:** the three reading rules; lost-only (filters not re-checked); the failure
  table.
- **Sites:**
  - the spiral order and its legality terms;
  - the `on` column rule and its candidate rule;
  - one structure per voxel;
  - the meaning of `covered {}`;
  - Build targets keyed by feature id.
- **Definitions:** UNCOVERED (living beacons, awake or dormant); a lost vent and a spent seam.
- **Companion rules:** no stacking (the same column); walk-in, with the timeout bounding the walk.
- **Sealed files:** "the sealed file holds the authored text".

## Amendments this makes elsewhere

- §2.7 item 11 (the placement range), for walk-in, and spec section 5's wording.
- §2.7 item 63 (world totals become per-seat room).
- Spec section 10's step table (walk-in, the reading rules, the failure codes) and section 13's chip
  text.
- Spec section 6's Build and Mine rows (`on`, `covered`, `seam_choice`).
- The proto comments listed under the first contract pull request.
