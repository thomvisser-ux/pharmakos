<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Changelog

Each release's notes name its tag and commit, and are the section below headed with the version
string the zips carry at that commit (`xtask/src/package.rs`'s `VERSION`;
`.github/workflows/release.yml`). The skeleton's development tags, `v0.1.0-dev.<n>`, share the
section below until the wk-35.5 release.

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

- A treasury in `$` with one grid in `kW`: a beacon's deploy is paid as it starts and refunded in
  full if it cannot finish, recycling refunds half of what the asset is still worth, and a deficit
  darkens beacons in the brownout order.
- Settlement at each recap, with the catch-up income band, and finite seams dug to a depth the
  playbook sets.
- The Build mandate's full settings, with build drones fabricated for the work in hand.
- Targeting by description: "the nearest heat vent you do not cover yet", ranked by travel when
  the step starts, with the editor's chips, the map's vent menu and the Lull's "This round"
  sentence.
- The verifier's money and time checks: the `$` and `kW` projection, the interface times, and a
  lower bound on walking against the coming segment.
- A first Lull of 10 minutes and later ones of 5, and Easy playing under the new rules.
