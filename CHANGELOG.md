<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Changelog

Each release's notes name its tag and commit, and are the section below headed with the version
string the zips carry at that commit (`xtask/src/package.rs`'s `VERSION`;
`.github/workflows/release.yml`). Versions are stage-based before 1.0, the minor number the stage
(decisions-log item 228): `0.N.0` closes stage N, `0.N.P` is a fix to it, `0.N.0-mK` is a
milestone build inside stage N, `1.0.0` is v1, and between tags `main` carries the stage being
built with `-dev`. The development tags `v0.1.0-dev.<n>` share the
`0.1.0-dev+skeleton` section.

## 0.4.0-dev

S4, Signals, in progress: builds from `main` between the `v0.3.0` tag and S4's first
milestone tag. Nothing a player sees has changed since 0.3.0 yet; this section collects what
lands, and the milestone's own pull request renames it to that tag's version.

## 0.3.0

S3, authoring: the build S3's close carries (decisions-log items 224 to 226). Unsigned zips for
Windows x86_64 and Linux x86_64. It holds the three earlier stages as well:

- The walking skeleton: one local match end to end against the built-in operator on Easy, in the
  Godot client and headless, deterministic and replayable bit for bit on Windows, Linux and macOS.
- S1, the economy (decisions-log item 143): one power grid per seat, finite seams, the Build
  mandate's full settings, targeting by description, and the verifier's money and time checks.
- S2, destruction and combat (decisions-log item 155): walls, autocannons and mortars, raiders
  and demolition charges, the Defend and Attack mandates, craters, and repair and salvage of
  wrecks.

New in S3:

- **Probation**, the lobby's one-click match: a player against Easy on seed 20, the 3 / 5 / 8
  ladder, untimed Lulls, and a **Playtest mode** checkbox, off by default.
- The wizard's **guided first pick**: one template marked as the place to start, with the
  gateway's reason, and the other templates still listed.
- The **size meter** under the editor: the playbook's size units against the budget of 128.
- The **rule list**: each rule a sentence with chips you click and type into, the verdict on the
  row; the W0702 connector between a rule that can never fire and the rule that shadows it, and
  dragging to reorder; a condition built through the picker (family, predicate, parameters); and
  an entry the seat cannot use yet greyed with a lock and its reason.
- **Save and Load** of JSONC playbooks, comments kept; a file holding a construct outside the v1
  vocabulary (a `set_flag`, say) is refused on Load with its code and pointer.
- **The first licence**: running Generators on two vents at a settlement awards the Resonance
  Spire's licence, with a recap line and the first-licence memo. A `toward` step places a
  stepping-stone beacon toward a named vent. The Spire is queued at a beacon (one per beacon, the
  verifier refusing a second), widens the core's sphere when it is built, and the sphere returns
  to its base radius if the Spire is lost.
- The Ledger's **memos**: a power shortfall, the first licence and the commander's first death.
- The **playtest survey** and **Export playtest bundle**: the survey's three questions, the
  metrics and the client's diagnostics log in one file you send by hand, with no playbook, draft,
  notebook or cache file and no network.
- Pathing in the sim and the travel estimate the verifier and the editor show, certified within
  their gates; the verifier's FULL inspection within its time at the size budget.
- A Survey's scout count, a Mine's dig depth and its pillar spacing can be set to 0 by an edit
  row, and `clear_protected_areas`, `clear_protect`, `clear_avoid_areas` and
  `clear_probe_areas` empty a beacon's lists. **A hand-written file that wrote one of those
  three numbers as 0 in an edit row now sets it to 0** (before, the 0 did nothing), and in a new
  beacon's or a switched mandate's settings the 0 is now charged one field.

## 0.1.0-dev+skeleton

The walking skeleton: one match end to end, headless and in the client.

- A deterministic 20 Hz sim with an integer world, destructible voxel chunks and a per-tick
  state hash that replays bit for bit on Windows, Linux and macOS.
- Playbooks as typed data: the verifier's seal inspection, plan-core's canonical JSONC,
  and the Lull and Push rounds with beacons, mandates and programs.
- The Seat Gateway on localhost with per-seat tokens, the fog filter, saves and resume.
- The built-in operator on Easy as the opponent, with its templates in `library/`.
- The Godot client: the lobby, the watch rig, the playbook editor and its template wizard,
  and a Credits screen.
- `gamectl`: `verify`, `schema`, `docs`, `scenario run`, `seat doctor` and `host`.
- Unsigned zips for Windows x86_64 and Linux x86_64. No signing, no notarisation and no
  macOS build yet.

S1, the economy, added to the same development builds:

- One power grid per seat: a deficit sheds only a beacon whose shedding relieves it, and a
  priority raise on site relights a dark beacon when a lower one can be shed.
- A beacon's deploy is paid as it starts and refunded in full if it cannot finish.
- Finite seams, mined to a depth the playbook sets.
- The Build mandate's full settings, with build drones fabricated for the work in hand.
- Targeting by description: "the nearest heat vent you do not cover yet", ranked by travel when
  the step starts, with the editor's chips, the map's vent menu and the Lull's "This round"
  sentence.
- The verifier's money and time checks: the `$` and `kW` projection, the interface times, and a
  lower bound on walking against the coming segment.
- A first Lull of 10 minutes and later ones of 5, and Easy playing under the new rules.
