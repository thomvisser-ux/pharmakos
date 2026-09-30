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

So every CI job that runs the project, and every packaging step, runs
`godot --headless --path <project> --import` first. `cargo xtask stage-client` does it for
you, and `.github/workflows/ci.yml`'s client leg stages the project with that command and
nothing else. The same leg then builds `gamectl`, runs `scenes/watch_check.tscn` headless
against a real `gamectl host`, and checks that no `gamectl` process outlives the client.
`cargo xtask package` imports its staged copy of the project before it exports it. The
`clean launch (<os>)` jobs run no `--import`, on purpose: they hold no project, only the
exported build, whose extension list the package job's import wrote (decisions-log item
117 (8)).

## Layout

| path | what |
| --- | --- |
| `project.godot` | the four settings G1 found decide whether a frame number means anything; the main scene is `scenes/boot.tscn` |
| `pharmakos.gdextension` | item 73's shape: `gdext_rust_init`, `compatibility_minimum = 4.7`, `reloadable = false`, explicit Windows and Linux paths |
| `bin/` | the staging target. Git-ignored except for its `.gitkeep`; nothing built is committed |
| `fixtures/view_keyframe.jsonl` | a byte-identical copy of the gateway's keyframe golden, for the hostless vista shot; `crates/client-gdext/tests/vista_fixture.rs` keeps it identical. Excluded from both export presets, with every fixture (T21) |
| `fixtures/editor_check.jsonc` | the committed playbook the headless watch check opens, edits and submits: plan-core's canonical form, qualifying both on the golden seed's seat 0 and against `gamectl verify`'s reference seat |
| `fixtures/editor_check.expected.jsonc` | what the watch check must submit: `editor_check.jsonc` with the check's one map action appended, as plan-core's `patch_text` (the function behind `patch_plan`) writes it |
| `fixtures/out_of_vocabulary.json` | the verifier's own E0003 case, byte for byte: the file Load must refuse with a code and a pointer |
| `fixtures/needs_a_fix.jsonc` | the verifier's own E0108 case: a file with one machine-applicable Fix |
| `fixtures/rows_report.json` | a `VerifyReport` of four committed verifier diagnostics, for the rows scene; the pull request that moves this fixture re-renders `tests/golden/vista/expected.rows.png` from CI's Linux run, as `tests/golden/vista/README.md` says |
| `fixtures/instantiate_suggested.json` | a byte-identical copy of the gateway's `tests/golden/gateway/instantiate_suggested/expected.response.json` (seat 0's `instantiate_template{suggested: true}` answer for Hold & Build), for the wizard scene and the watch check's pin (the pull request that moves the golden always re-copies it, and re-renders `tests/golden/vista/expected.wizard.png` from CI's Linux run only when something the wizard's first page draws moved: its label, value, mark or why, and a template comment is none of them; `tests/golden/vista/README.md` says how); `crates/client-gdext/tests/wizard.rs` keeps it identical and pins its pages. **The lane that changes `library/` moves that golden and so owns this copy too**: re-copy it, and update the page pins, in the same pull request |
| `scenes/boot.tscn` | the main scene: `--scene=<res path>` after `--` sends the run there, otherwise to the lobby |
| `scenes/lobby.tscn` | the game at the skeleton: starts `gamectl host`, watches the match (T16), and edits the seat's orders (T19) |
| `scenes/vista.tscn` | the vista: the bridge, the camera rig, the entity markers, the Pall |
| `scenes/vista_shot.tscn` | the vista golden's scene: the keyframe fixture with **no host running** |
| `scenes/watch_check.tscn` | the headless-driven run against a real `gamectl host` (CI's `client` job): the watch rig, the editor, the wizard, the meter, the camera's keys (`_camera_keys`: W held in the focused notes box leaves the camera still, and moves it once a right-click on the view has released the focus), the first click on the panel (`_first_click_lands`: one injected click on Submit, a Fix button and Save reaches its handler exactly once, with the rows redrawn under the press, with a map menu open and after a panel scroll), and a resume by a fresh client |
| `scenes/rows_shot.tscn` | the validation rows drawn from `fixtures/rows_report.json` with no host; `cargo xtask screenshot` compares it with `tests/golden/vista/expected.rows.png` |
| `scenes/wizard_shot.tscn` | the wizard's first page drawn from `fixtures/instantiate_suggested.json` with no host; `cargo xtask screenshot` compares it with `tests/golden/vista/expected.wizard.png` |
| `scenes/client_check.tscn` | T12's headless acceptance scene |
| `scenes/smoke_check.tscn` | the shipped smoke check (T21): the real lobby idles, opens and closes the credits overlay, starts a New match and reaches the Lull; the one check the export keeps |
| `scenes/credits.tscn` | the credits overlay the lobby's Credits button adds: the licences by area, the notices file's name, the CC BY attributions (none yet) and Godot's notices from the engine |
| `export_presets.cfg` | the Windows Desktop and Linux export presets (T21); see "The export" below |
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

## The demo's findings F1, F3 and F6 (S1's plan, task `fixc`)

**F1, the first click.** At the skeleton's demo the first click on Fix or Submit after the
panel had changed or scrolled did nothing, and the second worked (decisions-log item 126
(3)). Reproduced headless, with injected input: the editor redrew its rows on every change
the bridge reported, so a Fix button pressed while an answer came back was freed before its
release, and the click reached no handler (`button_down` once, `pressed` never). The rows are
now rebuilt only when they change; a press on the panel closes the map menus; a click the
map takes releases the GUI focus; the lobby's containers, drawn over the panel, ignore the
mouse; and Submit and Fix say "Submitting..." and "Fixing..." at once. The watch check's
`_first_click_lands` holds all of it. To reproduce by hand, with the window focused:

```sh
godot --path godot -- --log-input --gamectl=<target>/debug/gamectl[.exe] --root=<repository>
Pharmakos.exe -- --log-input      # the packaged game
```

The editor then prints `[editor] <button>: button_down` for every press and `[editor]
<button>: pressed` for every click its handler took, on Submit, Fix and Save, and the lobby
prints `[lobby] _input: ...` for every mouse button it sees. A swallowed click shows as a
press with no `pressed` after it, or as a click the lobby saw and no button did.

**F3, a second instance.** A second `Pharmakos.exe` hung at the demo with an empty log. The
empty log is explained: a release build flushed its standard output and
`user://logs/godot.log` only on an error or a clean exit, so any instance ended by hand
leaves an empty log. `project.godot` now sets `application/run/flush_stdout_on_print`, so
the next hang leaves the lines printed before it. The hang itself was not reproduced (the
pull request of task `fixc` records every attempt), so it has no fix yet (S1's plan,
decision 19). Two instances do share `user://`: the second renames the first's live
`godot.log` aside when it starts, and both then write the new one.

**F6.** The chooser alone shows before a match: the watch strip and the editor's panel appear
once the host has announced one, and the status line names the speed only in a Push. The
client check's two expected refusals (`decode_result` of an unknown field and
`transpose_chunk` of a short chunk) print a note, not an `ERROR`. They are the two `ERROR`
lines the demo found: the smoke check has no negative probe and prints none, run from the
editor or from an export, while the client check printed exactly these two; and the project's
name is the game's, so a developer's checks and the packaged game share one `user://logs/`.

## The editor (T19, pull request 2)

Pull request 2 adds four things to the panel, each drawn as the gateway answered it
(`docs/design/skeleton-plan-w6-notes.md` section A4, as decisions-log item 112 amends it):

- **The template wizard** (`scripts/wizard.gd`): one button per template `list_templates`
  lists; the one opened shows one page per parameter `instantiate_template{suggested:
  true}` lists, in its order, with the template's label, the value as the raw JSON the
  gateway wrote (a duration stays game milliseconds), a mark where the value is the built-in
  operator's suggestion, and the operator's "why" as it came. A value the player sends goes
  exactly as typed, as the one explicit value, so the other pages keep the operator's value
  and mark (the watch check asserts that live for every page the gateway marked, and
  `tests/wizard.rs` holds the wizard's code to never converting a value); a refusal is shown as the gateway wrote it. **Use** puts the gateway's
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
match ends the lobby forgets it (decisions-log item 113 (11)), and its status line says so.
After a refused Resume the chooser stays hidden and the line stays remembered, so picking
New match means restarting the client (a PLACEHOLDER in `scripts/lobby.gd`, with the
New/Resume layout, S6). The only files the client writes are the player's JSONC, that one
remembered line, and the watch check's own `user://` files.

One check drives the lobby with a host: the shipped smoke check (below), which presses New
match and never Resume. The watch check drives `host_link.gd` directly and never touches the
remembered line. What the lobby may do with a remembered match is pinned by its source
(`crates/client-gdext/tests/godot_project.rs`: it writes only the remembered line, once the
host has announced; it names and resumes only through `host_link.gd`'s helpers; it forgets
on ENDED), and T22's run sheet ("quit in a Lull, resume from the lobby", w6 notes A5) is
where a person sees it run.

## The export (T21)

`cargo xtask package` (decisions-log item 117) builds this platform's unsigned zip: the
client library with `--profile release-client` and `gamectl` with `--release` (on Windows
with `+crt-static`), a copy of this project without `.godot/` and `bin/`, the library staged
into the copy, `--import` on the copy, the smoke check in the copy under the editor, then

```sh
godot --headless --path <copy> --export-release "Windows Desktop" <out>/Pharmakos.exe
godot --headless --path <copy> --export-release Linux <out>/Pharmakos.x86_64
```

with Godot 4.7.2's release export templates installed. `export_presets.cfg` holds two
presets, x86_64, release, `embed_pck` off, so the export writes the executable, its `.pck`
and the client library beside it. Its exclude filter names `fixtures/*`, `*_shot.tscn`,
`*_shot.gd` and the client and watch checks' scenes and scripts by name, never a `*_check`
wildcard: Godot applies the exclude filter after the include filter, so a wildcard would
drop the smoke check and no include could bring it back. `godot_project.rs` pins that, and
`package` byte-scans the `.pck` for it. The Windows preset has Godot edit the executable's
resources itself (`application/modify_resources`, no rcedit): product name and file
description the lockup, product version `0.1.0-dev+skeleton`, file version `0.1.0.0`.

Godot rewrites `export_presets.cfg` when the editor saves presets, so the file keeps no
comment, and its PLACEHOLDERs live here:

- PLACEHOLDER: the product name and file description are the lockup, *PHARMAKOS: THE
  SEALED ORDER*, until the owner confirms the name (plan T21); `config/name` stays
  `Pharmakos`, so the window title is the bare word and `user://` does not move. OWNER,
  with the org.
- PLACEHOLDER: the icon (Godot's own until the art pass), the company name and the
  copyright field are empty. OWNER, with the org, before the owner publishes a release.

The smoke check runs the same way from the editor and from the export:

```sh
godot --headless --path <project> -- --scene=res://scenes/smoke_check.tscn \
    --gamectl=<gamectl> --root=<repository>
Pharmakos/Pharmakos.exe --headless -- --scene=res://scenes/smoke_check.tscn
```

It lets the real lobby idle for ten frames, opens the credits overlay through the Credits
button and closes it with Back, presses New match, waits for the host's announce and the
Lull, checks the bridge is the real class and caught no panic, frees the lobby and waits
for the host to exit, then prints one `[smoke] OK` line (or `[smoke] FAIL <reason>`) and
exits 0 or 1. An exported build finds `gamectl` beside the executable, and its root — the
folder holding `rules/` and `library/` — is the executable's folder
(`host_link.gd`'s `find_root`). Like the watch check, a run is a real New match: it writes
`user://last_match.txt` and leaves a match folder in the private match cache.
`cargo xtask package` points its own in-editor run's user folders at the emptied
`<target>/package/data/`; a run by hand points `APPDATA` and `LOCALAPPDATA` (on Linux
`XDG_DATA_HOME` and `HOME`) at a scratch folder.

Before T21 the exported lobby crashed on its second idle frame: the lobby pumps its host
link every frame, and before New match the link read a pipe that did not exist yet, which
the editor's debug VM logs as a SCRIPT ERROR per frame and the release template's VM does
not survive. `host_link.gd`'s `pump()` now reads nothing before `start()` has spawned the
host, and `godot_project.rs` pins it.

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

Windowed, like the rows shot. `cargo xtask screenshot` renders it on CI's Linux run and
compares it with `tests/golden/vista/expected.wizard.png` (decisions-log items 110 (4) and
116 (6)(h)). It draws the
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

Windowed, like the vista shot. `cargo xtask screenshot` renders it on CI's Linux run and
compares it with `tests/golden/vista/expected.rows.png` (decisions-log items 110 (4) and
116 (6)(h)). Headless without `--shot=`, the
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
