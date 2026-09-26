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
first. `cargo xtask stage-client` does it for you, and `.github/workflows/ci.yml`'s client leg
stages the project with that command and nothing else. The same leg then builds `gamectl`,
runs `scenes/watch_check.tscn` headless against a real `gamectl host`, and checks that no
`gamectl` process outlives the client.

## Layout

| path | what |
| --- | --- |
| `project.godot` | the four settings G1 found decide whether a frame number means anything; the main scene is `scenes/boot.tscn` |
| `pharmakos.gdextension` | item 73's shape: `gdext_rust_init`, `compatibility_minimum = 4.7`, `reloadable = false`, explicit Windows and Linux paths |
| `bin/` | the staging target. Git-ignored except for its `.gitkeep`; nothing built is committed |
| `fixtures/view_keyframe.jsonl` | a byte-identical copy of the gateway's keyframe golden, for the hostless vista shot; `crates/client-gdext/tests/vista_fixture.rs` keeps it identical. Excluded from any export preset (T21) |
| `fixtures/editor_check.jsonc` | the committed playbook the headless watch check opens, edits and submits: plan-core's canonical form, qualifying both on the golden seed's seat 0 and against `gamectl verify`'s reference seat |
| `fixtures/editor_check.expected.jsonc` | what the watch check must submit: `editor_check.jsonc` with the check's one map action appended, as plan-core's `patch_text` (the function behind `patch_plan`) writes it |
| `fixtures/out_of_vocabulary.json` | the verifier's own E0003 case, byte for byte: the file Load must refuse with a code and a pointer |
| `fixtures/needs_a_fix.jsonc` | the verifier's own E0108 case: a file with one machine-applicable Fix |
| `fixtures/rows_report.json` | a `VerifyReport` of four committed verifier diagnostics, for the render-only rows scene |
| `fixtures/instantiate_suggested.json` | a byte-identical copy of the gateway's `tests/golden/gateway/instantiate_suggested/expected.response.json` (seat 0's `instantiate_template{suggested: true}` answer for Hold & Build), for the render-only wizard scene and the watch check's pin; `crates/client-gdext/tests/wizard.rs` keeps it identical and pins its pages. **The lane that changes `library/` moves that golden and so owns this copy too**: re-copy it, and update the page pins, in the same pull request |
| `scenes/boot.tscn` | the main scene: `--scene=<res path>` after `--` sends the run there, otherwise to the lobby |
| `scenes/lobby.tscn` | the game at the skeleton: starts `gamectl host`, watches the match (T16), and edits the seat's orders (T19) |
| `scenes/vista.tscn` | the vista: the bridge, the camera rig, the entity markers, the Pall |
| `scenes/vista_shot.tscn` | the vista golden's scene: the keyframe fixture with **no host running** |
| `scenes/watch_check.tscn` | the headless-driven run against a real `gamectl host` (CI's `client` job): the watch rig, the editor, the wizard, the meter, and a resume by a fresh client |
| `scenes/rows_shot.tscn` | the validation rows drawn from `fixtures/rows_report.json` with no host: render-only, looked at and not compared until T20 |
| `scenes/wizard_shot.tscn` | the wizard's first page drawn from `fixtures/instantiate_suggested.json` with no host: render-only, looked at and not compared until T20 |
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

## The editor (T19, pull request 1)

`scripts/editor.gd` is the playbook editor: a panel on the right of the lobby and a route
and a placement ghost on the map. It is one more client of the gateway on the seat
connection the vista already holds, and a thin one. Load and Save read and write the
player's own JSONC file byte for byte; a file is opened only after QUICK has seen it, and an
out-of-vocabulary construct is refused with the verifier's code and pointer. Click one of
your beacons for Visit & change, Go here or Recycle; click the ground for Go here or Place
beacon (the ghost shows QUICK's verdict for that click); Alt-click a beacon to make the
step's target a selector. Every edit is a JSON Patch the gateway applies, QUICK runs on
every edit and FULL after 600 ms idle, and the rows' Fix buttons apply the verifier's own
machine-applicable patches. The route is the gateway's `estimate_route`, drawn as a polyline
with each leg's travel time in game milliseconds as it came back; there are no dashed legs
at the skeleton. The notebook, draft continuity, Submit and Ready go through the gateway
too. Every sentence the client writes itself is in `scripts/strings.gd`.

What the editor sends and when is the bridge's (`crates/client-gdext/src/editor.rs` and
`rig.rs`): the calls share the seat token's rate budget with the vista's polls, and Ready
waits behind any submission still on its way.

## The editor (T19, pull request 2)

Pull request 2 adds four things to the panel, each drawn as the gateway answered it
(`docs/design/skeleton-plan-w6-notes.md` section A4, as decisions-log item 112 amends it):

- **The template wizard** (`scripts/wizard.gd`): one button per template `list_templates`
  lists; the one opened shows one page per parameter `instantiate_template{suggested:
  true}` lists, in its order, with the template's label, the value as the raw JSON the
  gateway wrote (a duration stays game milliseconds), a mark where the value is the built-in
  operator's suggestion, and the operator's "why" as it came. A value the player sends goes
  exactly as typed, as the one explicit value, so the other pages keep the operator's value
  and mark; a refusal is shown as the gateway wrote it. **Use** puts the gateway's
  `playbook_jsonc` into the editor byte for byte, as one edit Undo takes back, and QUICK, the
  route, the rule list and FULL follow. Nothing in the client names a template, a pointer or
  a label, so a `library/` change needs no client change (`tests/wizard.rs`).
- **The rule list** (`scripts/rule_list.gd`): `render_plan`'s prose for the text on screen,
  one read-only row per line, its accessible name its line. No sentence is parsed; chips are
  S3's.
- **The meter**: `get_economy_forecast`'s four numbers (`$`, supply, draw and headroom in
  `kW`) put into `scripts/strings.gd`'s frame as they came. Headroom is the gateway's, never
  supply minus draw. The bridge's rig polls it when a Lull opens and whenever the match moved
  in a Push.
- **Draft continuity through `get_draft`**: when `list_drafts` shows this round's `carried`
  draft, the editor fetches it and opens it, every time, so a client that restarted after a
  resume opens last round's playbook as surely as one that submitted it.

The lobby now asks first: **New match** names the match `local-<unix seconds>-<pid>` through
`scripts/host_link.gd`'s one helper (the one clock read in the client's scripts, which names a
match and computes nothing), so it never reuses the id of an older match whose save is
still in the private match cache. Once the host has announced, the lobby remembers the
six-field config line it wrote, in one `user://` file with no token in it. **Resume last
match**, shown only when a line is remembered, writes that line plus `resume`; the gateway
checks it against the save and resumes into the Push a sealed save began or the Lull a quit
left. A refusal shows the host's own standard-error text, and nothing is retried. When the
match ends the lobby forgets it (decisions-log item 113 (11)). The only files the client
writes are the player's JSONC, that one remembered line, and the watch check's own `user://`
files.

## The watch check's two hosts, and the folders it leaves

The watch check (above) now plays two seats: this one and seat 1, which the built-in
operator plays. A one-seat match is decided at its Push's first tick, because the last seat
standing wins (`crates/sim/src/runner.rs`, `MatchState::decide`), so it would have no round 2
to quit in and resume. After round 1's recap it continues into round 2's Lull, idles there
past the gateway's read timeout, closes the host's standard input (the host saves the Lull
and exits), changes scene so the bridge, its rig and its editor are rebuilt, and starts a
second host with the same six fields plus `resume`. There is one final `[watch-check] OK`
line, naming both hosts' process ids.

The check names its match `watch-check-<unix seconds>-<pid>` through the same helper as the
lobby (decisions-log item 113 (13)): `gamectl host` keeps its matches in the real per-user
private match cache, and a local re-run that reused a process id would be refused because
the earlier run's save is still there. **Each local run therefore leaves one match folder
behind** — `%LOCALAPPDATA%\Pharmakos\matches\watch-check-*` on Windows,
`$XDG_DATA_HOME/pharmakos/matches/` (default `~/.local/share`) on Linux, and
`~/Library/Application Support/Pharmakos/matches/` on macOS. The client cannot remove it:
it has no access to the cache, which is the gateway's (w6 notes, decision C9). Delete old
`watch-check-*` folders by hand when they pile up; nothing prunes them until S6's save
browser.

## The wizard shot

```sh
godot --path godot --resolution 1280x720 -- --scene=res://scenes/wizard_shot.tscn --shot=<png>
```

Windowed, like the rows shot, and **render-only** at T19: the shot is taken and looked at, and
T20 commits and compares it (decisions-log item 110 (4); the w6 notes' A5). It draws the
wizard's first page from `fixtures/instantiate_suggested.json`: the page's label, its raw value in the field, the operator's mark, the why, and Use and Close.
Headless without `--shot=`, the scene checks the page's, the field's and the mark's
accessible names and quits.

## GraphEdit in Godot 4.7.2 (spec section 19's open item)

**GraphEdit is not marked experimental in the installed Godot 4.7.2**, and neither are
`GraphNode`, `GraphElement` or `GraphFrame`, nor any of their members. Evidence, read from the
engine itself rather than from memory: the editor's own class-reference cache for 4.7
(`%LOCALAPPDATA%\Godot\editor_doc_cache-4.7.res`, written by the installed
`4.7.2.stable.official.ed1daf0bf` from the reference compiled into it), loaded by a script
through `ResourceLoader`, lists 1076 classes; the classes that carry an `experimental` key
there include `Compositor`, `CompositorEffect` and `SkeletonModification2DPhysicalBones`, and
the four graph classes carry none, on the class or on any method, property, signal, constant
or theme item. `godot --headless --doctool` is no evidence either way: run outside the
engine's source tree it writes the class list with empty descriptions and no marks at all,
and `--dump-extension-api-with-docs` carries descriptions but has no experimental field.
Nothing in the client depends on GraphEdit (spec section 19); this is reported, not used.

## The rows shot

```sh
godot --path godot --resolution 1280x720 -- --scene=res://scenes/rows_shot.tscn --shot=<png>
```

Windowed, like the vista shot. It is **render-only** at T19: the shot is taken and looked at,
and T20 commits and compares it (decisions-log item 110 (4)). Headless without `--shot=`, the
scene draws the rows, checks each row's accessible name is its sentence, and quits.

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
seam, and every `.gd` under `godot/`, for exactly that
(`the_editor_makes_no_time_arithmetic_of_its_own`). No fog logic and no reveal toggle either: what a seat sees is the
gateway's decision, and `tests/unlock.rs` scans every script for the words that would start
one.

## What is not here yet

Chips on the rule list, drag reordering and the pickers are S3's; unit display of wizard
values, the typed parameter catalogue, the draft and save browsers and the real lobby are
S6's; the `.vox` models and the art pass are S6's too. Entities are drawn as placeholder
primitives until then.
