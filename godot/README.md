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
| `fixtures/vocabulary.json` | a byte-identical copy of the gateway's `tests/golden/gateway/get_vocabulary/expected.response.json` (seat 0's `get_vocabulary` answer), for the hostless picker scene and the client's tests; `tests/vocabulary.rs` keeps it identical and reads from it every predicate, mandate, step and blueprint name the client may never say. **The lane that moves that golden re-copies it** in the same pull request |
| `fixtures/beacon_core.json` | a byte-identical copy of `tests/golden/gateway/get_beacon_core/expected.response.json` (seat 0's core beacon, its settings and no targets), for the Visit & change rows' tests; `tests/vocabulary.rs` keeps it identical. The lane that moves the golden re-copies it |
| `fixtures/beacon_build.json` | a byte-identical copy of `tests/golden/gateway/get_beacon_build/expected.response.json` (a Build beacon holding targets, a wall and a described vent among them), for the removal row's tests; `tests/vocabulary.rs` keeps it identical. The lane that moves the golden re-copies it |
| `fixtures/beacon_lists.json` | a byte-identical copy of `tests/golden/gateway/get_beacon_lists/expected.response.json` (a Build beacon holding two protected areas: a list row with its lines and its clear, S3's `edlists`), for the clear's tests and the client check's Visit & change leg; `tests/vocabulary.rs` keeps it identical. The lane that moves the golden re-copies it |
| `fixtures/rule_list_render.json` | a byte-identical copy of the gateway's `tests/golden/gateway/demo_expand_and_mine_render/expected.response.json` (the Expand & Mine demo's `render_plan` answer, chips and all), for the hostless rule-list scene; `crates/client-gdext/tests/godot_project.rs` keeps it identical. **The lane that moves that golden re-copies it** in the same pull request, and re-renders the rule list's shot once `xtask` takes it |
| `fixtures/get_status_probation.json` | a byte-identical copy of `tests/golden/gateway/get_status_probation/expected.response.json` (seat 0's `get_status` in a Probation match's first Lull: the preset and the seat's seal), for the hostless lobby shot; `crates/client-gdext/tests/godot_project.rs` keeps it identical and the rig's tests read it. **The lane that moves that golden re-copies it** in the same pull request |
| `fixtures/feed_ledger_memo.json` | a byte-identical copy of `tests/golden/gateway/feed_ledger_memo/expected.response.json` (a feed page carrying one of the Ledger's memos), for the lobby shot's event list; `crates/client-gdext/tests/event_list.rs` keeps it identical and pins the memo's row. The lane that moves the golden re-copies it |
| `fixtures/recap_probation_round.json` | a byte-identical copy of `tests/golden/gateway/recap_probation_round/expected.response.json` (a Probation round's recap: the licence sentence in its prose, one memo), for the lobby shot's recap; `godot_project.rs` keeps it identical and the rig's tests read it. The lane that moves the golden re-copies it |
| `fixtures/recap_probation_ended.json` | a byte-identical copy of `tests/golden/gateway/recap_probation_ended/expected.response.json` (the match's last recap: every standing award in the gateway's final words), for the rig's tests; `godot_project.rs` keeps it identical. The lane that moves the golden re-copies it |
| `fixtures/list_templates_guided.json` | a byte-identical copy of `tests/golden/gateway/list_templates_guided/expected.response.json` (seat 0's `list_templates` in a Probation match's first Lull: one template marked as the guided first pick with the gateway's sentence), for the template-list shot; `godot_project.rs` keeps it identical. Its summaries quote `library/`, so it carries both licences by its own `REUSE.toml` line, as the golden does. **The lane that moves that golden re-copies it** |
| `fixtures/view_built.jsonl` | a byte-identical copy of `tests/golden/gateway/view_built/expected.keyframe.jsonl` (seat 0's keyframe with a finished Resonance Spire: the core's grown ring), for the ring shot and the view's tests; `godot_project.rs` keeps it identical. The lane that moves the golden re-copies it |
| `playbooks/new.jsonc` | what New puts on screen: kind PLAYBOOK, an empty route, and the verifier's own `on_death` and `fallback` stubs, so one map step makes it sealable. It starts a player's own file, so it carries no header and no comment (comments round-trip into the player's text); it is MIT OR Apache-2.0 by its own `REUSE.toml` entry (the owner's ruling of 2026-11-17). Both export presets pack it by their include filter, `playbooks/*.jsonc` |
| `scenes/boot.tscn` | the main scene: `--scene=<res path>` after `--` sends the run there, otherwise to the lobby |
| `scenes/lobby.tscn` | the game at the skeleton: starts `gamectl host`, watches the match (T16), and edits the seat's orders (T19) |
| `scenes/vista.tscn` | the vista: the bridge, the camera rig, the entity markers, the Pall |
| `scenes/vista_shot.tscn` | the vista golden's scene: the keyframe fixture with **no host running** |
| `scenes/watch_check.tscn` | the headless-driven run against a real `gamectl host` (CI's `client` job): the watch rig, the editor, the wizard, the meter, the camera's keys (`_camera_keys`: W held in the focused notes box leaves the camera still, and moves it once a right-click on the view has released the focus), the first click on the panel (`_first_click_lands`: one injected click on Submit, a Fix button and Save reaches its handler exactly once, with the rows redrawn under the press, with a map menu open and after a panel scroll), the name Save as... offers (`_save_as_offers`: the opened file's, the saved file's under a name of its own, and none after a template or a carried draft; the native dialog's overwrite prompt is the package hand check), the map in a Push (`_refused_in_the_push`: a click opens the menu as before, still open after a redraw, and its ghost, its Go and a submission are refused at once), New then one Go here qualifying at QUICK (`_new_then_go_here`), and a resume by a fresh client |
| `scenes/rows_shot.tscn` | the validation rows drawn from `fixtures/rows_report.json` with no host; `cargo xtask screenshot` compares it with `tests/golden/vista/expected.rows.png` |
| `scenes/picker_shot.tscn` | the condition picker drawn from `fixtures/vocabulary.json` (a byte copy of the gateway's golden) with no host: the families, predicates and parameters as sent, one locked entry greyed with the gateway's reason; `cargo xtask screenshot` compares it with `tests/golden/vista/expected.picker.png` |
| `scenes/rule_list_shot.tscn` | the rule list drawn from `fixtures/rule_list_render.json` (a byte copy of the gateway's Expand & Mine render golden) with no host, its chips and one conflict connector; `cargo xtask screenshot` compares it with `tests/golden/vista/expected.rule_list.png` |
| `scenes/ring_shot.tscn` | the beacon's ring with no host: the vista over `fixtures/view_keyframe.jsonl` (the core's ring at the rules table's base) and then `fixtures/view_built.jsonl` (grown by a standing Resonance Spire), the camera opened on the core; without `--shot=` a check that each ring is drawn at the radius the view sent. `cargo xtask screenshot` compares it with `tests/golden/vista/expected.ring.png` |
| `scenes/templates_shot.tscn` | the template list with no host: `fixtures/list_templates_guided.json` through the bridge's decode and the panel's own `fill_templates`, the guided pick tinted with the gateway's sentence under it; without `--shot=` a check that it is drawn as sent. `cargo xtask screenshot` compares it with `tests/golden/vista/expected.templates.png` |
| `scenes/lobby_shot.tscn` | the lobby's Probation words with no host: the New match button, the status line's preset, the event list with the Ledger's memo, and a Probation round's recap with its memo and licence sentence, each drawn by `scripts/lobby.gd`'s own functions from the bridge's reading of a byte copy of a gateway golden; without `--shot=` a check of the same. `cargo xtask screenshot` compares it with `tests/golden/vista/expected.lobby.png` |
| `scenes/playtest_shot.tscn` | Playtest mode with no host: the chooser's check button switched on, the line that says what it records and that nothing leaves the machine, and the end-of-match survey with Export playtest bundle, each drawn by `scripts/playtest.gd` itself over the bridge's own recorder; without `--shot=` a check that the line and the survey show, a too-long answer is refused in the string table's words, and switching off clears the recorder. `cargo xtask screenshot` compares it with `tests/golden/vista/expected.playtest.png` |
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
the arrows, Q and E, the wheel, which zooms on what is under the cursor, the right mouse
button), a follow-commander toggle, the live
event list, 1x, 2x and 4x, and Skip. There is no pause (decisions-log item 108 (3)). The map
opens on this seat's own commander (at the core in round 1), close enough to read, and drawn
in the middle of what the editor's panel leaves of the window; with no commander in the view
it frames the whole map, as the Whole map button does (S3's `ed3b`, a P6a finding). The
Lull ends when every seat is ready or its timer runs out; Ready is this seat's `set_ready`,
and Continue leaves the recap. What each connection sends and when — the pacer, the host
clock reported four times a second outside a Push, the keep-alive — is the bridge's
(`crates/client-gdext/src/rig.rs` and `pacer.rs`), not GDScript's.

The same run with no human, headless, is CI's live acceptance:

```sh
godot --headless --path godot res://scenes/watch_check.tscn -- \
    --gamectl=<target>/debug/gamectl[.exe] --root=<repository>
```

## The Probation match (S3's `prob`, part 1)

The lobby hosts Probation (`scripts/host_link.gd`'s `PRESET`): the New match button names it,
"New match: Probation", from the string table's `preset_<name>` row fed that one constant,
and once a match is announced the status line names the preset the gateway says it hosts
(`get_status`'s `preset`, read by the bridge's rig at the admin connection's first call and
the seat connection's open), through the same rows. A hosted preset that is not the one New
match asked for is said as a refusal; a resumed match is named by the gateway alone. Nothing
in the client says what a preset does.

A Resume the host refuses - a line remembered before the preset existed has six fields, and
the gateway refuses it naming the field - shows the host's refusal as it came and offers the
chooser again, with New match and no Resume. The lobby reads only that a Resume was refused
before the host announced; the remembered line stays until New match announces and replaces
it. By hand: write a six-field line (the seven fields of a New match's line less the last)
into `user://last_match.txt`, launch, press Resume, and see the refusal and a chooser offering
New match only; New match then starts the match.

In a Probation match's first Lull the template list marks the one template the gateway picked
for a first match: its button is tinted, a mark with no words, and the gateway's own sentence
("Start here: ...") is drawn under it
as sent; the other templates stay offered. The list is asked for at every Lull's opening and
replaced whole, so the mark is not drawn once the gateway stops sending it. The Ledger's memos
are drawn in the event list under "The Ledger" in place of the event's kind, and on the recap
under their own heading, each as the gateway wrote it; the licence sentence is the recap's own
prose, drawn whole. Each own beacon carries a flat ring at the sphere radius the view sends,
the rules table's base grown by a standing Resonance Spire; another seat's beacon is sent no
radius and draws none.

The headless watch check's Probation leg (`_run_probation`) resumes a six-field line first,
sees it refused and starts the lobby's own line through the same link, then asserts that
`get_status` names `probation`, that round 1's first `list_templates` marks exactly one
template with a sentence, and that the panel draws the mark and the sentence as sent.

## Playtest mode (S3's `prob`, part 2)

The lobby's chooser carries a **Playtest mode** check button (`scripts/playtest.gd`). It is
off at every launch and is not remembered: nothing is collected unless the tester turns it on
in that session, and turning it off again drops everything it kept. Off, it costs nothing: no
call reaches the bridge. While it is on, a line under the event list says so, says what is kept
and says that nothing leaves the computer.

**What it keeps**, in memory only, in the bridge (`crates/client-gdext/src/playtest.rs`): per
Push, the wall time the window had focus and the time it did not, or that the Push was skipped
(a skipped Push carries no times); per Lull, the wall time used up to this seat's Ready or the
Lull's end; the count of Pushes and of skipped ones; speed changes by speed; camera moves by
kind (a free-look key starting a move, a wheel notch, a right-button turn, Follow); the
verifier's error rows shown, by code, once per text revision; the gateway's refusals, by code;
and the client's own diagnostics log, a line per event named from a closed list. Every Push and
Lull row names its match by a count from 1, never the match's id. A step between two frames
longer than 2 s (the window stalled, the machine slept) is not attention: it is kept apart as a
gap and counted. A Push or Lull still open when the client leaves its match (a new match, a
host that stopped) is kept, timed to the last frame drawn in that match, as `match_left`.

**What it never holds:** a playbook, a draft, the notebook, the private match cache or replay,
a match id, a seed, a token, a port, a path or a user name. Its inputs are typed facts and
codes from closed sets (the gateway's error codes, the verifier's code shape, the write
failures' names); a refusal's message, which can quote a player's text, is dropped. The raw
`user://logs/godot.log` and `client-<pid>.log` are never read. The one free text is the
survey's third answer, the tester's own words, which goes into the bundle exactly as written,
as the survey screen says beside the box.

**The survey and Export appear whenever Playtest mode is on and no match is running**: once a
match has ended, and whenever the chooser is shown, after a host gave up or stopped. The session
a playtest most wants is often the one that broke, so it can be exported as it stands. "Play
another round?" (yes / no), "Did you understand why your playbook did what it did?" (yes /
partly / no) and "What did you most want to change during a Push?" (free text, with a count
against its cap). Nothing of the survey is kept while it is filled in: the answers go into the
bundle with Export, as the screen shows them then, and a question left alone is sent as
`unanswered`. An answer past its cap refuses the whole export, writes no file, and says so
until the answer changes.

**A match that stopped early** (its host gave up or stopped): keep Playtest mode on (switching it
off drops what was kept); the chooser comes back with the survey and Export under it, and the
bundle holds the stopped match, its open Push or Lull marked `match_left`. A tester may instead
start a New match first; the bundle then holds both.

**Export playtest bundle** opens the operating system's save dialog, offering
`pharmakos-playtest.zip` and no folder of ours. The bridge builds one zip and writes it whole,
by the same atomic replace as Save, to the file the tester picked and nowhere else; the line
under the survey names that file. The zip holds exactly `manifest.json`, `survey.json`,
`metrics.json` and `diagnostics.log` (`bundle_format` 1; the recording is absent until S7).
No time of day, no path and no build id is in it; its times are wall microseconds, and the
log's `t=` is milliseconds since Playtest mode was switched on. Nothing is sent anywhere: the
tester reads the four files and sends the zip by hand. The client opens no network but its
localhost link to the host it spawned (`crates/client-gdext/tests/no_network.rs`).

The headless watch check switches Playtest mode on for its first match, plays round 1 with the
window's focus lost and regained in the Push, exports to a scratch folder with the survey and
reads the bundle back; the headless client check drives the real lobby's check button, the
cap's refusal typed key by key, the export, a stand-in bridge that counts calls while off, and
the camera's turn after a lost release; and `scenes/playtest_shot.tscn` draws the line and the
survey with no host.

## The editor (T19, pull request 1)

`scripts/editor.gd` is the playbook editor: a panel on the right of the lobby and a route
and a placement ghost on the map. It is one more client of the gateway on the seat
connection the vista already holds, and a thin one. New puts the empty playbook the client ships (`playbooks/new.jsonc`, packed by both export
presets' include filter) on screen as one edit Undo takes back, and the checks say what it
still needs. Load and Save read and write the
player's own JSONC file byte for byte; a file is opened only after QUICK has seen it, and an
out-of-vocabulary construct is refused with the verifier's code and pointer. Click one of
your beacons for Visit & change, Go here or Recycle; click the ground for Go here or Place
beacon (the ghost shows QUICK's verdict for that click, and the menu the click opened carries
the same words on its first line and greys Place beacon while the gateway says the spot is
illegal). A beacon's menu offers its target
every time: this beacon, or a selector (Alt held pre-selects the nearest own beacon). Every edit is a JSON Patch the gateway applies, QUICK runs on
every edit and FULL after 600 ms idle (while a check is on its way the verdict line and the
size meter wear a checking look, and when its answer arrives both are highlighted until the
player's next key, click or wheel notch: no timer, nothing flashes), and the rows' Fix buttons apply the verifier's own
machine-applicable patches. The route is the gateway's `estimate_route`, drawn as a polyline
with the whole route's and each leg's travel as the gateway rendered it (`travel_text`, the
register's S3-18; a figure sent with no text is drawn as its raw game milliseconds and says so); there are no dashed legs
at the skeleton. The notebook, draft continuity, Submit and Ready go through the gateway
too. Every sentence the client writes itself is in `scripts/strings.gd`.

What the editor sends and when is the bridge's (`crates/client-gdext/src/editor.rs` and
`rig.rs`): the calls share the seat token's rate budget with the vista's polls, and Ready
waits behind any submission still on its way.

Since S1's stage demo (decisions-log item 139 (2), lane `fixd`): Save writes, and Save as...
offers, the file the text on screen was opened from or last saved to, and no name after a
template's playbook or a carried draft (saving over a file that exists asks first, in the
save dialog); planning is the Lull's, so outside one the map offers no menu, an ask is
refused at once with the reason, and a check that will not come says why instead of
"Checking..."; and when the Lull runs out and the gateway files the safe playbook, the
panel says the orders on screen were not sealed, and says "Ready to seal" only in a Lull.

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
empty log is explained: a release build flushed its standard output and its log file only on
an error or a clean exit, so any instance ended by hand leaves an empty log.
`project.godot` now sets `application/run/flush_stdout_on_print`, so the next hang leaves the
lines printed before it. The hang itself was not reproduced (the pull request of task `fixc`
records every attempt), and the owner closed F3 with no guard (decisions-log item 139 (1),
ruling 11).

**Each instance's log (finding N, decisions-log item 139 (2)).** Two instances share
`user://`, and the engine's one `user://logs/godot.log` is rotated by every start, under any
instance already writing it, so that file only ever tells the latest start. It stays on,
because on a double-clicked Windows game it is the only record of a start that fails before
the main scene runs. Beside it the main scene adds `scripts/instance_log.gd`: each instance also
writes `user://logs/client-<process id>.log`, whole from boot, and a start keeps the newest
five (on Linux and macOS that can unlink the log of an instance still running, if five later
instances have started since; `instance_log.gd` says why that is accepted). The smoke check
asserts the file holds the boot line.

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
- **The rule list** (`scripts/rule_list.gd`; S3's `ed3b`): `render_plan`'s prose for the text
  on screen, one row per line, its accessible name its line, each line cut by the bridge at the
  chips `render_plan` placed on it. No sentence is parsed. A chip is a link: a number opens a
  field within the bounds the gateway sent, an enum its values as sent, and a reference a pick
  on the map (the next map click; Escape stops), and a step label or a rule id a menu of the
  labels or rule ids the playbook itself holds, in file order (whether the pick is legal is the
  gateway's check), each edit one JSON Patch `replace`. A duration is drawn as the gateway wrote
  it and not edited (it waits on a display unit from the wire). A step's or a rule's row
  drags onto another of its own list: one JSON Patch `move` to that rule's place, so a rule
  dragged down lands after the rule it is dropped on, and one dragged up before it (RFC 6902
  `move` removes, then inserts at the target index). The two rules a W0702 row names
  are joined by a connector in the list's margin, with the verifier's sentence under the later
  one, and the gateway's refusal of an edit shows under the rule it was of.
- **The condition picker** (`scripts/condition_picker.gd`; S3's `ed3b`, part 2): family, then
  predicate, then its parameters, every one `get_vocabulary`'s as sent (read once per Lull), a
  locked family or predicate greyed with the gateway's reason and not pickable. **Use** asks the
  bridge for one JSON Patch at the condition slot: the predicate's skeleton under its name, sent
  with exactly the op the gateway names for that slot (`add` where nothing stands, `replace`
  where something does, a group's item in place); its parameters are then the new rendering's
  chips. The rule list draws one button per slot the gateway sent (`render_plan`'s
  `rules[].slots`), in its words ("when, item 1 of all", "step 1, skip if"), on empty and leaf
  slots only: a group (`all`, `any`, `not`) is no pick target, its items are.
- **Visit & change** (S3's `ed3b`, part 2): the beacon menu's Visit & change submenu, beside
  S1's three priority rows, lists from `get_beacon`'s detail one row per setting (its title and
  value now, its editor by its shape), Switch mandate, Add a build target (a blueprint, then a
  click on the ground for its anchor; Escape stops), Remove a build target, and Queue a
  structure, every mandate and blueprint as `get_vocabulary` sent it and a locked one greyed
  with its reason. Each pick is one route step visiting the beacon with that one row. The detail
  describes the beacon as the Lull sees it; what an earlier step would leave is not projected.
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
remembered line, each instance's own log, and the checks' own `user://` files.

One check drives the lobby with a host: the shipped smoke check (below), which presses New
match and never Resume. The watch check drives `host_link.gd` directly and never touches the
remembered line. What the lobby may do with a remembered match is pinned by its source
(`crates/client-gdext/tests/godot_project.rs`: it writes only the remembered line, once the
host has announced; it names and resumes only through `host_link.gd`'s helpers; it forgets
on ENDED), and T22's run sheet ("quit in a Lull, resume from the lobby", w6 notes A5) is
where a person sees it run.

## The economy and targeting UI (S1's plan, task `ui`)

Every word and number below is the gateway's, drawn as it came; the client times and
schedules, and decides nothing (AGENTS.md section 3 rule 4).

* **The Lull's countdown.** Round 1's Lull counts down from the rules table's
  `match.first_lull_ms` and every later one from `match.lull_ms` (`scripts/mesher_rules.gd`
  carries both; `tests/godot_project.rs` pins them to `rules/rules.v1.json`). The status line
  shows no countdown while the admin connection's footer says `_status.untimed`, which it does
  until the gateway has been told one, and a Lull with no length is reported with no
  `remaining_ms` at all, never as a timer at 0 (decisions-log item 135; the register's S1-11).
  **The lobby hosts Probation** (decisions-log item 160 (4)): its Lulls have no countdown by
  rule, which the footer says as `_status.untimed_by_rule`, so the rig starts none, reports
  only the time spent, and the status line shows none; Ready ends the Lull. A `custom` match
  (the headless watch check's main legs, or a remembered line naming it) still counts each
  Lull down as above.
* **A Lull that ran out.** When a timed Lull's countdown is spent the rig reports it at 0
  before it calls `end_lull`, so the gateway can say the Lull ran out rather than that Ready
  ended it. After a stall it first catches up in steps no larger than the gateway takes
  (60 s), and the 0 counts only once the gateway answers it: a 0 lost with its connection is
  sent again. A 0 the gateway refuses is sent again at most three times
  (`rig::ZERO_REFUSALS`), at the clock's cadence; then the rig gives it up, ends the Lull
  without it, and the status line shows the gateway's refusal until the next Lull.
* **The recap.** In a recap and after the match ends, the lobby's column shows `get_recap`'s
  prose: the settlement's, the shortfall's and the "found nothing" lines are its sentences.
* **This round.** The panel shows the Lull's "This round: ..." sentence off `get_briefing`'s
  prose, read every Lull and after every accepted seal (item 133 (3) (i)). With no sentence
  (nothing sealed yet, the briefing not read yet, or a seal that reads the map nowhere) the
  heading is hidden: the client words no claim about the orders of its own.
* **The chips.** For every vent or seam the playbook names or describes, what it reads now,
  how far, and the next candidate, from `resolve_refs` for the text on screen.
* **The vent click.** A click on a heat vent offers "Place beacon covering this vent" (a name)
  and "the nearest vent you can cover" (a description), both every time; Alt held puts the
  description first (S3's `ed3b`: at P6a a real Windows window never delivered Alt with the
  click). With `-- --log-input`, every map click logs `event.alt_pressed`, whether the Alt key
  is held, Shift and Ctrl. The vents are those `get_map_summary` lists this Lull.
* **The meter.** Beside the four numbers: the next BMI, labelled as the BMI at the band held
  now, ignoring committed spend (item 135 (2) (g)); the committed spend in a Lull; and that
  supply and draw leave out each beacon's base draw, which its key-core supplies (S1-31). A
  figure the answer leaves out is not drawn.
* **E0601's checkbox.** "Allow dormant beacons" ticks in the verifier's own patch and clears
  the option; the next report says what it costs.

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
description the lockup, the product version `xtask/src/package.rs`'s `VERSION` and the file
version its numeric core plus `.0`, which `package.rs`'s tests hold the preset to (decisions-log
item 228; AGENTS.md §9, "Versions and tags").

Godot rewrites `export_presets.cfg` when the editor saves presets, so the file keeps no
comment, and its PLACEHOLDERs live here:

- PLACEHOLDER: the product name and file description — OWNER, with the org. They are
  the lockup, *PHARMAKOS: THE SEALED ORDER*, until the owner confirms the name (plan T21);
  `config/name` stays `Pharmakos`, so the window title is the bare word and `user://`
  does not move.
- PLACEHOLDER: the icon, the company name and the copyright field — OWNER, with the org,
  before a release. The icon is Godot's own until the art pass; the other two are empty.

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

## The watch check's three hosts, and the folders it leaves

The watch check (above) plays two seats: this one and seat 1, which the built-in
operator plays. A one-seat match is decided at its Push's first tick, because the last seat
standing wins (`crates/sim/src/runner.rs`, `MatchState::decide`), so it would have no round 2
to quit in and resume. After round 1's recap it continues into round 2's Lull, idles there
past the gateway's read timeout, closes the host's standard input (the host saves the Lull
and exits), changes scene so the bridge, its rig and its editor are rebuilt, and starts a
second host with the same six fields plus `resume`. Once that host has exited it changes scene
again and starts a third host on the lobby's own line (the Probation seed and preset), whose
Lull it watches for four seconds: clock reports answered, none carrying a countdown, the
footer untimed by rule, no countdown shown and nothing refused. There is one final
`[watch-check] OK` line, naming the three hosts' process ids.

The check names its match `watch-check-<unix seconds>-<pid>` through the same helper as the
lobby (decisions-log item 113 (13)): `gamectl host` keeps its matches in the real per-user
private match cache, and a local re-run that reused a process id would be refused because
the earlier run's save is still there. **Each local run therefore leaves two match folders
behind**, the first match's (which the resumed host reuses) and the Probation leg's
(`watch-check-probation-*`) — `%LOCALAPPDATA%\Pharmakos\matches\watch-check-*` on Windows,
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

Groups (`all`, `any`, `not`) and an "Add rule" control are not built in S3 (a group's items
are picked as leaves; a new rule comes from a template or the file); unit display of wizard
values, the typed parameter catalogue, the draft and save browsers and the real lobby are
S6's; the `.vox` models and the art pass are S6's too. Entities are drawn as placeholder
primitives until then.
