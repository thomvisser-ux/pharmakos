<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Targeting: how a playbook names where and what

**Adopted 2026-09-29 (decisions-log §2.7 item 127 (12)).** The owner asked for three rounds of
investigation and then for the recommendation to be adopted. Where this page and a §2.7 item
disagree, the item wins; where this page and the spec disagree, this page is the later decision and
the spec is updated to match (the amendments are listed at the end).

## Why

At the walking skeleton's demo a carried playbook placed a beacon at the same voxel every round
(item 126 (3), F4). The owner: a positional playbook "only makes sense for round 1"; "the most
obvious strategy … is build things based on the location of things that are already visible"; and
the vocabulary must be forward-thinking, because it "will impact much of the scripting and ai and
ai-assist to come" and "is hard to undo once playbook files use it". The same vocabulary has to serve
a first-time player in the editor, the built-in operator, v1.1's external scripts, v1.2's live
agents and the advisor, without becoming a language (AGENTS.md §2 and §11).

The design came out of three research passes (the scripting and AI future, prior art in programmable
strategy games and LLM game agents, the codebase's own binding times), a panel of three independent
designers (elegance, future authors, rigour), two adversarial critics (the player and scope;
contracts, determinism and fog) and a final verification pass that walked ten scenarios through it.

## The model

A target is a **name** or a **description**.

- A **name** is a fixed id: `b_NN` for a beacon (per seat from S1, below), `v_<x>_<y>` for a vent and
  `s_<x>_<y>` for a seam.
- A **description** is a pick from one closed catalogue per kind: a rank and a few flat filters, for
  example "the nearest vent not covered by my spheres". No variables, no arithmetic, no offsets, no
  loops, one level deep. Tags stay the only author-chosen names.

The sealed file always holds the text as the author wrote it, never what it resolved to. A carried
playbook therefore reads its descriptions again every round, which is what makes carrying it
sensible.

## Three reading rules

1. **A step reads all its targets when it starts**, including the ones it writes into a new beacon's
   initial settings or an interface row, and keeps them until it ends. A step checks its bound
   features at each decision; a lost one fails the step with `target_lost`, and no row ever commits a
   lost target.
2. **A beacon keeps the target it was given until you change it on site.** A description a beacon
   holds reads again only when its target is lost (a vent destroyed, a seam spent); a name idles when
   its target is lost.
3. **A condition checks now**, at every evaluation.

Named exceptions, unchanged: item 13's Attack target, pinned at seal (S2); the reflex's and the
fallback's `safest`. A mandate's own `seam_choice` is a choice inside the Mine program, held until the
seam is spent, and uses the same "nearest"; a Mine beacon may dig a seam other than the one it was
placed to cover.

A held target fails only when it is **lost**. Filters are not re-checked while a target is held,
because the holder's own act (covering a vent, building on it, mining a seam dry) must not disqualify
its own target.

## Failure

One code, `no_target`, whether the thing is hidden, absent or someone else's, so a guess reveals
nothing; with it `target_lost`, `no_legal_site` and, for a walk that cannot arrive, `no_path`. Each is
a step failure that `on_fail` handles; a condition over an empty reference is false; nothing
substitutes silently. The existing `not_own` answer, which tells a seat whether an unseen enemy
beacon is alive, is replaced by `no_target`.

## Names

- **Vents and seams** are named by their generation anchor column, the patch or disc centre, with no
  z (craters change z): `v_120_88`. Players see "Heat vent (120, 88) · rich". An anchor id reveals no
  count and no order, so it stays safe under S3's fog, and a guessed id answers `no_target` exactly as
  a hidden one does.
- The map generator refuses duplicate anchors and overlapping footprints with a `MapError`, so the
  ids are unique by construction.
- The feature table is a pure function of the seed, the rules and the occupied seats (item 38 lays no
  features in empty zones). It is regenerated on restore, pinned in the mapgen digest golden, and not
  hashed per tick. A feature's liveness is derived by scanning its footprint when a holder reads it
  (9 voxels for a vent, at most about 150 for a seam), so it adds no hashed column and needs no hook
  in `set` or `crater`. A vent is lost when no exposed vent material lies under its column.
  `power.rs`'s `one_vent` moves onto the table.
- **Beacons** become per seat (the leak ruling of item 127 (13)): a seat's core is always `b_00`, so a
  template can name it anywhere; the table room becomes per seat, for beacons, Build targets and
  structures alike.

## Descriptions (S1)

`FeatureRef { oneof ref { string feature_id = 1; VentPick vent = 2; SeamPick seam = 3; Covered covered = 4; } }`

- `VentPick` and `SeamPick` each carry a `rank` (S1: `NEAREST`) and a `coverage` filter (`ANY` or
  `UNCOVERED`); `RANK_UNSPECIFIED = 0` and `COVERAGE_UNSPECIFIED = 0` are verifier errors.
- **UNCOVERED** means outside all of the seat's own live spheres. It never reads another seat's
  state.
- `BeaconRef` stays as it is; the picks are per kind, and "one catalogue" means one rank vocabulary
  and one resolver, not one message.
- A description resolves from the seat's own state and the static features only, so it is fog-fair
  and the seat order of the decision phase cannot change a result.

### Nearest

**Nearest = the least estimated travel from the holder to the feature, over the terrain the seat
knows. Unreachable candidates are skipped. Ties go to the lowest anchor column.**

- The metric is the item-61 estimator's integer cost, the number the editor already shows as an ETA,
  so what a chip says and what the sim picks agree. It prices climbs, pits and craters, and never
  picks a target the commander cannot reach.
- The origin is the commander's column when a step starts; for a description a beacon holds, the
  beacon's anchor column.
- It measures to the feature's anchor column, or, when that cannot be stood on or is not connected,
  to the first standable connected footprint column in a fixed order.
- A feature with no connected footprint column is not a candidate; when nothing remains, the answer is
  `no_target` and the recap says so ("3 matched, none reachable").
- Ties: `(cost, anchor y, anchor x)`, the pathfinder's own node order. Integers only; total.
- Cost: at most one estimate per candidate (six or fewer per kind on an S1 map), pruned by the octile
  lower bound, only when a holder starts. S1 measures it against the tick budget.
- Feature `NEAREST` is refused in conditions in S1 (a condition reads every 250 ms, and re-ranking
  every evaluation thrashes); the arm stays reserved.
- `BeaconRef.nearest` still means octile distance. Aligning it with this rule is booked before the
  public v0.1.

## Sites

`Location` gains two arms, `FeatureRef on = 10;` and `FeatureRef covering = 11;`, and its reserved
range narrows to 12–49.

- **`covering`**, legal only in `PlaceBeaconStep.at`: the sim ranks the candidates and takes the
  first one it can cover. Around that feature it walks a fixed spiral (squared xy distance from the
  anchor, then distance to the commander at step start, then y, then x), bounded by the sphere radius
  and excluding every feature footprint, and takes the first legal column whose sphere holds the
  feature's `on` column. The operator's private `site_for` moves into the sim as this rule.
- **`on`**, legal only in a Build target's or a structure's anchor: the pick ranks only features whose
  `on` column lies inside the target beacon's sphere (for an initial row, the new site's sphere),
  measured from that beacon. The `on` column is the anchor column if it is free, otherwise the next
  live footprint column in (y, x) order. "Free" ignores the beacon being written to. `coverage` under
  `on` is a verifier error.
- **`covered {}`**, legal only under `on` in a Build target inside the `initial` of a `place_beacon`
  whose `at` is a `covering` arm: "the feature this beacon was placed to cover". It binds as a name at
  deploy and idles like one when its feature is lost.
- Build targets made through `on` are keyed by feature id: `RemoveBuildTargetRow` accepts
  `on {feature_id}`, and carry-over replaces by feature id. The claim check (`anchor_is_claimed`) is
  scoped to the seat.
- No claim column is needed in S1: one commander per seat means one step reads targets per seat per
  decision, and descriptions read only the seat's own state. Two beacons of one seat re-reading at
  once is an S2 question (craters), answered by binding in ascending beacon id with the per-seat
  target table as the claim.

## Companion changes

- **Walk-in.** `place_beacon` and `interface` walk to their target first. The step's `timeout_ms`
  bounds the walk up to arrival. A sealed-in or unreachable site fails `no_path`. The editor and
  `estimate_route` draw the implied leg. `skip_if` replaces the old implicit "only if already there"
  guard. `commander.placement_range_voxels` loses its role, which amends item 11.
- **Restart keeps the placed beacon.** A step that death, the reflex or a handler clears after its
  deploy resumes as a visit to the beacon it placed, with the rows it had left, and reads nothing
  again, so it never pays for a second beacon.
- **No stacking.** A site where one of the seat's own beacons stands is illegal (`no_legal_site`),
  which also cures carried fixed-voxel files.
- **Bugs fixed with it.** `estimate_route` sent every selector leg to the seat's first beacon
  (`crates/gateway/src/surface/knowledge.rs`), and `crates/sim/src/interpreter/cond.rs`'s module doc
  says selectors resolve once per step, though a condition's resolve every evaluation.

## Surfaces

- **One resolver.** A read-only trait in the sim, reached with `default-features = false`, is
  implemented by the tick's view and by the verifier's `Scope`. `Scope` gains the features (id, kind,
  grade, standing z at the anchor, a live bit) and the commander's position, once, so every
  `report_hash` golden moves once.
- **Site previews** come from the gateway's `resolve_refs` over the frozen world, internal in S1 and
  on `ADVISOR_METHODS`, published with v1.1. The editor cannot reach the sim (`CLIENT_WALL`) and the
  operator's library depends on proto alone, so both ask the gateway.
- `get_map_summary.features` (S1-48) lists every feature with its id, kind, grade, coverage by the
  seat and travel. `estimate_route` accepts `covering` and returns the site, so Easy needs one call
  per candidate and keeps no second site algorithm. The operator derives no ids itself.
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
- **Messages.** On re-seal the Lull says what this round will do ("this round: new beacon near
  (150, 20), $ 60", or "no vent you can cover"). The recap names why a step found nothing.

## What the map means for it (a finding, not a rule)

On the golden seed, seat 0's start vent is covered in round 1. The nearest uncovered vents after that
are contested ones about 80 voxels away, beyond a single beacon's 48-voxel reach, so a carried
"cover the nearest vent" finds nothing to cover in rounds 2 and 3. That outcome is deterministic,
harmless and honest, and the Lull says so. Reaching those vents takes stepping-stone expansions. The
operator already has that logic ("one expansion out on the line towards it"). A `toward` site arm
(`Location` 12) is S1's plan's open question, not adopted here.

## Deferred, with their numbers reserved

- `RICHEST`, grade filters, `in_reach` and enemy-known filters (S3).
- Feature `NEAREST` in conditions, a risk-weighted rank, a `from` origin override, `toward`.
- The non-hashed `target_bound` event for replay readers, the fallback latch, publishing
  `resolve_refs` (v1.1), `extract_template` and rebind (v1.1).
- The seed withheld from seat tokens at S3 (item 127 (13)). A local agent with file access can still
  read `match.json`, and the docs say so.
- The estimator's connectivity oracle knows true connectivity under S3's fog, a small leak S3 decides.

## Contract pull requests (S1)

1. **Proto:** `proto/**`, a scoped `buf.yaml` ignore for the narrowed reserved range, the generated
   tree, and the `get_schema` and docs goldens. It rewrites `Location`'s "one selector catalogue
   rather than two" comment.
2. **Determinism:** beacon ids and table room per seat, the snapshot and save version bump that
   refuses old saves (old private replays checked by `match.json`'s `gateway_version`), the new
   hashed state (a binding per description in a step's `PlanState`, each target's description and
   bound id, the seat's beacon ordinal), and every chain.
3. **Behaviour:** walk-in, restart-keeps-beacon, no stacking, the resolver, the sites. These move the
   chains too, so 2 and 3 land together and re-bless once.
4. **Docs:** the spec and the amendments below, and AGENTS.md: §3 rule 2's wording on previews, and a
   §5 sentence that a reserved range may be discharged by the thing it was held for.

## Irreversible once playbook files use them

The id spelling (`v_<x>_<y>`, `s_<x>_<y>`, per-seat `b_NN`); the field numbers and JSON names above;
the metric and tie-break chain of `NEAREST`; the three reading rules; the failure mapping; the spiral
order; lost-only (filters not re-checked); and "the sealed file holds the authored text".

## Amendments this makes elsewhere

- §2.7 item 11 (the placement range) and spec section 5's wording, for walk-in.
- Spec section 10's step table (walk-in, the reading rules, the failure codes) and section 13's chip
  text.
- Spec section 6's Build and Mine rows (`on`, `covered`, `seam_choice`).
- `proto/gp/v1/playbook.proto`'s `Location` comment.
