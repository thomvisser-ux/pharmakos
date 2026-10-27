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
producing test is `crates/verifier/tests/verifier.rs`. There is **one plain
fixture** — one seat, one snapshot, one scope — written out in that file's
`fixture_scope` and `fixture_snapshot`, and three variants of it, each changing
one thing for the cases that need it: `map_scope` (the map and the commander),
`short_scope` (a grid already short) and `segment_snapshot` (a coming segment),
described below. Every case not named for a variant is checked against the
plain fixture:

| Beacon | Side | Writ | At | Notes |
| --- | --- | --- | --- | --- |
| `b_01` | own | BUILD | 80, 11, 55 | marked as the seat's core (`is_core`) |
| `b_02` | own | MINE | 100, 20, 58 | tagged `east` |
| `e_01` | enemy, known | — | 300, 300, 40 | seen, not readable |

The core's name is the skeleton's. Since S1's targeting determinism (`tgt`) a
live seat's core is `b_00` and its beacons are numbered per seat; the fixture
keeps `b_01` marked as the core because the verifier reads `is_core`, never the
number, and because `crates/gamectl/src/seat.rs` writes this fixture a second
time for `gamectl verify`'s test, so renaming it here alone would split the two.

Sharing the fixture is deliberate: a case's job is to isolate **one
diagnostic**, and a per-case scope would make each report a function of two
things that changed instead of one. The one exception is an `I0003` note: from
S1 (S1-39, the plan's decision 11) an omitted `pace`, `seam_choice`,
`pillar_spacing` or `terraform` reads as its named default with that note, so a
case written before S1 that leaves one of them out carries an `I0003` beside the
code it isolates. The note is information, never changes whether the case
qualifies, and is not what the case is about.

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

and the commander at 82, 13, 55.

**The economy and the segment, for the estimate stage's cases (S1).** From
S1's `proj` the estimate stage reads the seat's treasury, supply and draw and
the coming segment's length, which spec section 3 puts in the frozen snapshot.
Two of its codes need a fixture the plain one is not, and each gets one change
and nothing else:

| List in `verifier.rs` | Fixture | The one change |
| --- | --- | --- |
| `SHORT_CASES` (`w0601_an_existing_shortfall`) | `short_scope` | supply 4 `kW` under a draw of 10, so the grid is already short |
| `SEGMENT_CASES` (`w0701_a_route_longer_than_the_segment`) | `segment_snapshot` | `coming_segment_ms` 180 000, a first round's three minutes |

The plain fixture's economy is a treasury of `$` 200, a supply of 10 `kW` and a
draw of 4, and its snapshot carries no coming segment, so no other case can
raise `W0601` or `W0701`.

Every other case keeps the plain fixture, because
`crates/gamectl/src/seat.rs`'s reference seat is that fixture written a second
time and `gamectl verify`'s test compares the worked example's `report_hash`
with the golden here. `crates/bench/src/quick.rs` restates all five fixtures
and their lists, and `crates/bench/tests/harness.rs` holds the restatement to
the reports here. A case is named after the code it isolates,
and a code that can be wrong in more than one shape gets more than one case —
`E0111` has three, because an unset choice on `Fallback.posture`, on
`Location.place` and on `BeaconRef.ref` are three different things for an author
to have done. Three cases are meant to pass with no error:

* `expand_east` — spec section 10's worked example, canonicalised. It must
  qualify and measure **6 size units** (decisions-log item 94's own worked
  example). From S1 it carries four `I0003` notes and nothing stronger: it
  omits `pace` on two moves and `seam_choice` and `pillar_spacing` on its Mine
  block, and S1's plan's decision 11 (the register's S1-39, ruled by item 128)
  reads each as a named default with a note. From S1's estimate stage it also
  carries an `I0001`: ten seconds of walking at least, on the one leg whose two
  ends the view names (from the deploy at 96, 11 back to `b_01`). If it ever
  gains a warning or an error, either the verifier or the spec's example is
  wrong, and the pull request has to say which.
* `cover_the_nearest_vent` — targeting's adopted spelling, "place a beacon
  covering the nearest vent you do not cover, and build a Generator on it",
  against the map: no diagnostic at all.
* `budget_128` — a playbook that sits **exactly on** the size budget, for the
  walled bench harness to time. Wall-clock time is illegal in
  `crates/verifier`, so nothing here times anything: `crates/bench`
  (`pharmakos-bench`, S1's `p1`, decisions-log item 131) is the producer, and
  `cargo xtask perf-alarms` publishes QUICK's p50 and p99 over every case here
  and over this one alone, as notices with no threshold until P1's gate sets
  one (AGENTS.md section 9 item 11).

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
  their QUICK reports do not. S1's `proj` filled the estimate stage, so FULL
  reports gained `E0601`, `W0601` to `W0603`, `W0701` and `I0001` where their
  cases call for them: an `I0001` on every case whose route has a leg between
  two places the view names, and the rest only on their own cases.
  `full_finds_what_quick_finds` holds in its S1 form: FULL's diagnostics start
  with QUICK's, what follows comes only from a FULL stage, and the one error it
  may add is `E0601` (spec section 11: "adding draw beyond supply is an error
  unless 'allow dormant beacons' is set").
* **Every `report_hash` moved in S1's targeting verifier, the reports whose
  diagnostics stayed the same included.**
  The seat's view widened to carry the features and the commander's position
  (`Scope::encode`), and `REPORT_HASH_DOMAIN` went from `gp.api.v1/report/1` to
  `/2` with it, so input 2's bytes moved for every case, the cases with no map
  included (an empty feature list and an absent commander are bytes too).
* **Every `report_hash` moved in S1's Build settings (`build`), and nothing
  else in any report did.** `SNAPSHOT_VERSION` went from 8 to 9 (each Build
  target's order and rotation, and each structure's rotation), and the version
  is the snapshot's first field, so input 2's bytes moved for every case. The
  `plan_fingerprint` of every case is unmoved: the fingerprint moved into the
  sim's `hash` module with its value held (the register's S1-42), and
  `crates/sim/tests/vectors.rs`'s `the_plan_fingerprint_is_unmoved` pins three
  of these files' values.
* **Every `report_hash` moved in S1's estimate stage (`proj`), the QUICK-only
  ones and the reports whose diagnostics stayed the same included.**
  `VERIFIER_VERSION` went from `0.1.0-skeleton` to `0.1.0-s1`, input 4 of the
  five: a FULL report from this build finds what the skeleton's empty stage
  could not, so it is not comparable with one from before.
* **Every `report_hash` moved in S2's first verifier lane (`vfy1`), the reports
  whose diagnostics stayed the same included.** `VERIFIER_VERSION` went from
  `0.1.0-s1` to `0.1.0-s2`, input 4 of the five: this build refuses, by a code
  of its own, every construct the sim refuses at the seal (`E0009` to `E0017`),
  a `scout_count` past a beacon's byte (`E0508`) and a seal the Ledger cannot
  price (`E0603`), so its reports are not comparable with S1's. Each new code
  has its own case, named for it. Three existing cases gained the new codes
  beside the one they isolate: `e0404_impossible_filter` (`E0015` and
  `E0016`: the sim refuses both filters until S3, so the warning's file no
  longer qualifies), `e0601_a_scout_count_past_every_supply` (`E0508` and
  `E0603`) and `e0601_found_before_an_unpriced_blueprint` (`E0010`). The
  report hash's length prefixes became checked rather than saturating
  (`HashError`); no input reaches `u32::MAX`, so that moved no encoding, which
  was checked by running the goldens under the old version string: no
  `report_hash` moved. `full_finds_what_quick_finds` holds in its S2 form: the
  errors FULL may add are `E0601` and `E0603`.
  Its review's fix pass added `E0018` (an enum number this build does not
  name, read raw rather than through prost's getter, which would answer the
  default) with its case `e0018_an_enum_number_this_build_does_not_name`, and
  `E0004`'s message now says a missing `schema_version` is missing rather
  than "0.0" (`e0004_no_schema_version`); of the committed reports only
  `e0004_unsupported_version`'s message moved, and no `report_hash` did.
* **Every `report_hash` moved in S2's second verifier lane (`vfy2`), the
  reports whose diagnostics stayed the same included.** Two of the five inputs
  moved. The seat's view gained its sightings with their ages
  (`Scope::encode`), and `REPORT_HASH_DOMAIN` went from `gp.api.v1/report/2`
  to `/3` with it, so input 2's bytes moved for every case (an empty sighting
  list is bytes too); and `VERIFIER_VERSION` went from `0.1.0-s2` to
  `0.1.0-s2.2`, input 4. Reports moved for three rules:
  * an omitted `roe` reads as RETURN_FIRE with an `I0003` note (S2's plan,
    decision 4), on every deploy's `initial` and every `set_mandate` that
    names a writ and leaves `roe` out;
  * a Build beacon whose `repair_threshold_pct` is 0 carries an `I0004` note
    (decision 38), under the same carriers;
  * `E0508`'s message names its field (`scout_count`, or from S2 Defend's
    `picket_scouts`), so `e0508_more_scouts_than_a_beacon_holds`'s message
    moved.

  Seventeen cases are new: one per new code (`w0502_a_protect_list_naming_one_asset_twice`
  among them, a harmless repeat being advice and never a refusal), a second
  `E0416` for a target's anchor outside its beacon's sphere, two `E0101`
  shapes for a wall's box (none, and a corner missing), and
  `e0003_a_go_code_launch`, which pins that `AttackLaunch`'s held go-code arm
  (`broadcast_received`, S4's) is refused by name. Its message, "not part of
  the v1 vocabulary", is decode's wording for every reserved name; for an S4
  arm it says less than the truth, a wording question for the string table.
  Every Build target with a voxel anchor and a named holder is now checked
  against that beacon's sphere too (`E0416`, the column test).
  A Defend case also carries `E0012`, an Attack case `E0013` and a wall case
  `E0019`, because the sim refuses those arms and that blueprint whole until
  S2's `dfnd`, `atk` and `site` lanes; the case still isolates its own code.
* **Every `report_hash` moved in S2's threat model's verifier lane (`vfy3`)**,
  because `VERIFIER_VERSION` went from `0.1.0-s2.2` to `0.1.0-s2.3`, input 4.
  Reports moved for three rules and two wordings:
  * a move paced `AVOID_KNOWN_THREATS` while the view holds no enemy beacon in
    live vision carries an `I0005` note (S2's plan, decision 12), which
    `expand_east` gains on its first move;
  * an omitted `roe` reads by writ (decisions-log item 148 (2)): FREE_FIRE on
    a Defend or an Attack beacon, RETURN_FIRE on the others;
  * a protected, probe or avoid area missing a corner is `E0101`, as a wall's
    box already was (item 146 (4));
  * `E0003`'s message says a reserved name is held back for a later stage or
    version rather than "not part of the v1 vocabulary", which was less than
    the truth for S4's names (both `e0003_*` cases moved);
  * the retired rows `E0012`, `E0014`, `E0017`, `E0019` and `W0501` say they
    are retired, on the pattern `atk` set for `E0013` (the catalogue only).

  Four cases are new: `i0005_avoiding_threats_with_none_sighted`,
  `i0003_a_defend_beacons_omitted_roe`, `i0003_an_attack_beacons_omitted_roe`
  and `e0101_an_area_missing_a_corner`. They were re-blessed over `main` after
  S2's `atk` merged, so the Attack case carries no `E0013`.

**The sightings, for the cases that read them (S2).** The cases named in
`verifier.rs`'s `SIGHT_CASES` (`e0418_an_attack_target_seen_too_long_ago`,
`w0703_a_reference_resting_on_an_old_sighting`) are checked against
`sight_scope`, the plain fixture with the seat's sightings added and nothing
else changed:

| Sighted | At | Age |
| --- | --- | --- |
| `e_01` | 300, 300, 40 | 240 000 ms, past the reach window's 180 000 |
| seat 1's commander | 290, 295, 40 | 30 000 ms |
* **Every `report_hash` moved again when this lane merged `main` after `ids`
  (#123), and nothing else in any report did.** `ids` widened the snapshot to
  `SNAPSHOT_VERSION` 10, and the fixture snapshot is input 2 of the five, so
  each case's bytes moved (E33: the second lane to merge re-blesses).

**S2's `site` (snapshot version 12, and `E0019` retired).** Every
`report_hash` moved, and nothing else did except where the base kit is named:
`site` widened the snapshot to `SNAPSHOT_VERSION` 12 (each beacon's Build
settings and paid drone work, each wall target's top), and the fixture snapshot
is input 2 of the five. It also built the base kit, so the sim no longer
refuses `autocannon`, `mortar` or `wall` and `E0019` is retired: its catalogue
row keeps its number and is raised by nothing (`none at the skeleton (retired by
S2's site, which built the base kit)`), the wall cases no longer carry it beside
their own code, and `e0019_a_base_kit_blueprint_before_its_lane`, whose one
diagnostic it was, is gone with its golden. The ground-dependent halves of a
wall box's checks -- whether `min.z` reaches the ground, the height over the
ground in every column, and the 3-D sphere test -- are the sim's, refused when
the row commits (`wall_off_the_ground`, `wall_too_low`, `wall_outside_sphere`),
because the verifier's view carries no terrain (decisions-log item 146 (4)).
