<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Changelog

Each release's notes name its tag and commit, and are the newest section below at that commit
(`.github/workflows/release.yml`). Sections run newest first, one per stage until the wk-35.5
release; the development tags, `v0.1.0-dev.<n>`, take the section that is newest when each is cut.

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
