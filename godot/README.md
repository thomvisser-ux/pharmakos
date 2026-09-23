<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# `godot/` — the client project

Godot **4.7.2 exactly**, with gdext **0.5.5 exactly** (decisions-log §2.7 item 73). A
version bump is determinism-adjacent — it re-runs the geometry golden — and goes through a
deliberate pull request.

`godot/` and `crates/client-gdext` are **one ownable unit** under AGENTS.md §6 (item 72):
one agent holds both at a time, because GDScript has no compiler to catch a merge.

## The one command

```sh
cargo xtask stage-client            # build the cdylib, stage it, import, byte-scan
cargo xtask stage-client --check    # the above, then run the headless client check
```

It reads the target directory from `cargo metadata` rather than assuming `target/`, copies
the library into `bin/`, byte-scans it for `gdext_rust_init`, and then runs
`godot --headless --path godot --import`.

## `--import` is not optional, and it is not a cache warm-up

A non-editor Godot run loads GDExtensions **only** from `res://.godot/extension_list.cfg`.
The editor writes that file when it scans the project, and `.godot/` is git-ignored — so on
a fresh checkout, which is every CI runner, the extension's classes instantiate as
**placeholders** and the first call on one fails. Locally this never shows, because the
editor has opened the project once. Spike G1 lost four runs to it (§10.12).

So every CI job and every packaging step runs `godot --headless --path godot --import`
first. `cargo xtask stage-client` does it for you; `.github/workflows/ci.yml`'s client leg
runs that command and nothing else.

## Layout

| path | what |
| --- | --- |
| `project.godot` | the four settings G1 found decide whether a frame number means anything; the main scene is `scenes/boot.tscn` |
| `pharmakos.gdextension` | item 73's shape: `gdext_rust_init`, `compatibility_minimum = 4.7`, `reloadable = false`, explicit Windows and Linux paths |
| `bin/` | the staging target. Git-ignored except for its `.gitkeep`; nothing built is committed |
| `fixtures/view_keyframe.jsonl` | a byte-identical copy of the gateway's keyframe golden, for the hostless vista shot; `crates/client-gdext/tests/vista_fixture.rs` keeps it identical. Excluded from any export preset (T21) |
| `scenes/boot.tscn` | the main scene: `--scene=<res path>` after `--` sends the run there, otherwise to the lobby |
| `scenes/lobby.tscn` | the game at the skeleton: starts `gamectl host`, watches the match (T16) |
| `scenes/vista.tscn` | the vista: the bridge, the camera rig, the entity markers, the Pall |
| `scenes/vista_shot.tscn` | the vista golden's scene: the keyframe fixture with **no host running** |
| `scenes/watch_check.tscn` | the headless-driven run against a real `gamectl host` (CI's `client` job) |
| `scenes/client_check.tscn` | T12's headless acceptance scene |
| `scripts/` | GDScript — **views and editor UI only** (AGENTS.md §3 rule 4) |
| `.godot/` | Godot's own import cache. Git-ignored, and written by `--import` |

## Running the watch rig

```sh
cargo xtask stage-client                         # build, stage, import
cargo build -p pharmakos-gamectl --bin gamectl   # the match host the lobby spawns
godot --path godot -- --gamectl=<target>/debug/gamectl[.exe] --root=<repository>
```

The lobby spawns `gamectl host` beside the executable unless `--gamectl=` or
`PHARMAKOS_GAMECTL` names another, writes the one config line on its standard input, reads
the one announce line, and opens the **admin** and **seat** WebSocket connections with the
two tokens it announced. The tokens live in `scripts/host_link.gd`'s variables and nowhere
else. Closing the window closes the host's standard input, which ends the host.

The watch rig is what spec section 3 names for a Push and nothing more: free-look (WASD or
the arrows, Q and E, the wheel, the right mouse button), a follow-commander toggle, the live
event list, 1x, 2x and 4x, and Skip. There is no pause (decisions-log item 108 (3)). The
Lull ends when every seat is ready or its timer runs out; Ready is this seat's `set_ready`,
and Continue leaves the recap. What each connection sends and when — the pacer, the host
clock reported four times a second outside a Push, the keep-alive — is the bridge's
(`crates/client-gdext/src/rig.rs` and `pacer.rs`), not GDScript's.

The same run with no human, headless, is CI's live acceptance:

```sh
godot --headless --path godot res://scenes/watch_check.tscn -- \
    --gamectl=<target>/debug/gamectl[.exe] --root=<repository>
```

## The vista shot

```sh
godot --path godot --resolution 1280x720 -- --scene=res://scenes/vista_shot.tscn --shot=<png>
```

Windowed, never `--headless` (the dummy renderer never fires `frame_post_draw`). The golden
itself is rendered on CI's Linux leg under xvfb and lavapipe; `tests/golden/vista/README.md`
says how, and what it shows.

## What GDScript may do here

Read what the bridge returns, and draw it. No game rule, no validation, no arithmetic on
`$`, `kW` or a duration — the editor "runs no validation or time maths of its own", it asks
the gateway, and `crates/client-gdext`'s `tests/no_arithmetic.rs` scans both sides of the
seam for exactly that. No fog logic and no reveal toggle either: what a seat sees is the
gateway's decision, and `tests/unlock.rs` scans every script for the words that would start
one.

## What is not here yet

The editor, the notes box and the real lobby are **T19**'s; the `.vox` models and the art
pass are S6's. Entities are drawn as placeholder primitives until then.
