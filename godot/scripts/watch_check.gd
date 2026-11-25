# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The headless-driven run against a REAL `gamectl host` (skeleton-plan-t16a-notes.md
# section A (4); T19's acceptance, skeleton-plan-w6-notes.md section A4 as decisions-log
# item 112 amends it): lobby -> the editor -> the wizard -> Ready ends the Lull -> 4x ->
# recap -> round 2 -> quit in the Lull -> a fresh client resumes it -> the lobby's own
# Probation match.
#
# It drives the same scene pieces the lobby does - the host link, the vista, the editor,
# the bridge's watch rig - with a script where the human would be, and asserts:
#
#   * the host announced, both connections opened and the first keyframe arrived whole;
#   * NEITHER CONNECTION DROPPED across an idle stretch longer than the gateway's
#     30-second READ_TIMEOUT: in a Lull the seat connection has nothing to ask, so what
#     keeps it alive is the keep-alive, and what keeps the admin connection alive is the
#     host clock it reports four times a second. The stretch is spent in round 2's Lull,
#     so round 1's Lull timer is left whole for the editing below;
#   * THE EDITOR (T19, pull request 1), on the same seat connection:
#       - Load REFUSES an out-of-vocabulary file (`fixtures/out_of_vocabulary.json`) with
#         the verifier's code and JSON Pointer, opens nothing, and strips nothing; the
#         refusal's row has its plain sentence as its accessible name;
#       - a file with a machine-applicable diagnostic (`fixtures/needs_a_fix.jsonc`) is
#         fixed by its Fix button, through `patch_plan` with the verifier's own patch;
#       - a committed playbook in plan-core's canonical form, `fixtures/editor_check.jsonc`,
#         opens, QUICK qualifies it, and it saves back BYTE FOR BYTE;
#       - a ground click (the map surface's own handler) brings a placement ghost back
#         with QUICK's verdict, which must be legal; the menu's Place beacon takes it, and
#         Undo gives the loaded bytes back;
#       - one map action through the map surface's own handlers (Alt-click on the seat's
#         own beacon, Go here, nearest own beacon) is applied through `patch_plan` with the
#         selector in the step, QUICK runs, the route is priced, and FULL runs after 600 ms
#         idle and qualifies;
#       - the notebook is saved through `save_notes` and a draft through `save_draft`;
#       - the playbook is submitted and accepted, the SUBMITTED BYTES EQUAL
#         `fixtures/editor_check.expected.jsonc` (plan-core's own patch of its canonical
#         form: see that file's note in godot/README.md), and `gamectl verify` accepts the
#         file the editor saved;
#   * THE METER (T19, pull request 2): drawn from one `get_economy_forecast` answer in the
#     Lull and one in the Push, its text equal to strings.gd's frame over that answer's
#     four fields - nothing else is compared, because no economy value is this check's to
#     pin;
#   * THE WIZARD, for every template `list_templates` lists (generic: no template is named
#     but the one pinned below):
#       - every page the gateway's answer lists is drawn, in order, with its label, its raw
#         value, the operator's mark where the gateway set it and the why as it came;
#       - one page's shown value is sent back explicitly, and the page then has no mark and
#         shows what was sent;
#       - Use puts the gateway's playbook into the editor byte for byte; one map action goes
#         through `patch_plan`; QUICK and FULL qualify; every rule-list row's accessible
#         name is its line; the submission is accepted and the SUBMITTED BYTES EQUAL THE
#         GATEWAY'S LAST TEXT (decisions-log item 112 (4)); the file is saved and
#         `gamectl verify` is run on it: its only errors may be the three the reference
#         seat view makes of this match's names and places, because `gamectl verify`
#         checks against that view, not this match (crates/gamectl/src/seat.rs): E0403 at
#         a placed site (its spheres are not this match's), E0412 at a named feature (it
#         holds no feature) and E0401 at a named beacon (its beacons are `b_01`, `b_02`
#         and `e_01`, and this match's core is `b_00`). The live FULL at submit is that
#         file's qualifying check;
#   * THE PIN: Hold & Build instantiated with every parameter
#     `fixtures/instantiate_suggested.json` lists typed explicitly (read from the fixture,
#     never written here) is the fixture's `playbook_jsonc` byte for byte. An explicit value
#     beats a suggestion, so the live operator cannot move it. It is instantiated only,
#     never used or submitted: its anchor is off the map, so QUICK would say E0402;
#   * THE CAMERA'S KEYS (decisions-log item 123 (2) 8), in round 1's Lull with follow off:
#     with the notes box focused, a held W (fed through Input.parse_input_event, as a
#     keyboard would) leaves the camera rig's focus where it was; a right-click on the 3D
#     view, which no control takes, releases the GUI focus, and the same W then moves the
#     rig, so the check is not vacuous. The window has the project's size while it runs
#     (a headless one is 64 pixels square, all editor panel), and the notes box's text and
#     the window's size are put back afterwards;
#   * THE FIRST CLICK LANDS (F1, decisions-log item 126 (3); `_first_click_lands`), in round
#     1's Lull at the project's window size: one click injected through
#     Input.parse_input_event, as a mouse would send it, on each of Submit, a Fix button and
#     Save, in three cases each - with the rows redrawn between the press and the release,
#     with a map menu open, and after a scroll of the panel - reaches that button's handler
#     exactly once (the editor's own counts of `button_down` and `pressed`). In the scroll
#     case a Fix button is clicked with the panel left scrolled; Submit and Save sit at the
#     panel's top, so the scroll takes them out of sight and the panel is wheeled back to
#     0 px before they are clicked: for those two the case is a scroll down and back up,
#     clicked at rest, not a click on a panel left scrolled. A press on the
#     panel closes the menu; Save with no file opens Save as...; the Fix is applied and the
#     submission is accepted;
#   * SAVE AS OFFERS THE TEXT'S OWN FILE (S1's stage demo, finding G; `_save_as_offers`):
#     the committed playbook's name once it is opened, the saved file's (a different name,
#     so the two cannot be told apart by luck) once it is saved, and no name at all after a
#     template's playbook is used or the carried draft opens; the native dialog's overwrite
#     prompt is not something a headless run can show, so it stays the package hand check;
#   * THE MAP'S ASKS ARE REFUSED AT ONCE IN A PUSH (finding K; `_refused_in_the_push`): a
#     left-click on the ground beside the commander, through the map surface's own handler,
#     opens the map's menu as it always did (item 139 (2): what the map offers in a Push is
#     unchanged), its ghost and its Go are refused at once with the Push's reason, nothing
#     the editor owes is left waiting, and a submission is refused at once. Its positive
#     control, `_map_click_opens_a_menu`, frames the whole map as the lobby does and makes
#     the same click in round 1's Lull, where the ghost is asked for and comes back;
#   * THE PANEL'S LINES (S3's `ed3a`; `_panel_lines`, items 139 (2) H, 143 (3), 155 (3) 2 and 3):
#     a template's playbook used, then Undo: the status line says "Undone." and not "Undo takes
#     it back", and Undo is disabled; Load opening another file closes the wizard; and the size
#     meter says what the last report counted, and stays on screen (`_one_click`);
#   * ESCAPE CLOSES THE MAP MENU (finding I; in `_map_click_opens_a_menu`): the key, through the
#     engine's input as a keyboard would send it, closes the menu the click opened;
#   * THE WHEEL ON THE REAL VIEW (item 155 (3) 4; `_real_wheel`): from the whole map, wheel
#     notches through the engine's input at a pixel whose ray the view's own pick answers with
#     ground zoom the camera in, and that ground stays under the pixel;
#   * Ready (`set_ready`) ends the Lull: the host is a two-seat match whose other seat the
#     built-in operator plays and readies at every Lull's start (T18), so this seat's Ready
#     completes every seat's, the host clock's answer says `all_ready`, and the admin
#     connection calls `end_lull` (skeleton-plan-t16a-notes.md section B, "T19" (6));
#   * the pacer at 4x plays the Push, and the admin connection's `_status` footer reaches
#     RECAP; the seat connection then catches up before the rows are counted, and the
#     recap is read once through `get_recap`, its prose round 1's (S1's plan, task `ui`);
#   * THE RESUME LEG: Continue leads into round 2's Lull, where the carried draft is opened
#     through `get_draft`; the host's standard input is closed (the host saves the Lull)
#     and the host exits; then the scene is changed, so the bridge, its rig and its editor
#     are rebuilt with nothing carried but the config line and the expectations (in
#     Engine metadata), and a new host is started with the same six fields plus `resume`.
#     Round 2's Lull comes back, the notebook comes back through `get_briefing`, the drafts
#     are listed, and the carried draft is opened through `get_draft` with the bytes of
#     round 1's sealed submission;
#   * THE PROBATION LEG (S3's `ed3b`, decisions-log item 160 (4)): once the resumed host has
#     exited, the scene changes again and a third host is started on the lobby's own line
#     (`HostLink.config_line` with its defaults: the Probation seed and preset). Its Lull
#     footer says `untimed_by_rule`, so the rig starts no countdown and sends no
#     `remaining_ms`, which a Probation gateway refuses with INVALID_ARGUMENT: across a
#     stretch of clock reports the gateway refuses nothing, the footer stays untimed and
#     the bridge's `timer`, the lobby's countdown, stays empty. Before that host, S3's `prob`
#     resumes a six-field line (one remembered before the preset existed), sees the host
#     refuse it before announcing, lets the refused host go (`host_link.gd`'s `reset`) and
#     starts the lobby's line on the same link, as the lobby's refused Resume does; in the
#     Lull, `get_status` names `probation` as asked, and round 1's first `list_templates`
#     marks exactly one template with a sentence, which the panel draws as sent. The first
#     host's legs hold `get_status` to `custom` and read the seat's `sealed_this_round`;
#   * THE PLANNING WIRE (S3's `fixq-c`; `_planning_wire`), in round 1's Lull against the real
#     gateway, on the committed playbook: a duration chip is a number typed in seconds, pre-filled
#     with the gateway's own text, and "3" typed into its field and sent lands as 3000 game
#     milliseconds by `set_value_text`; a choice chip's menu draws the gateway's titles; Add a
#     rule, clicked, lands a rule whose empty condition slot the picker fills with a group, which
#     lands too; Remove, clicked, takes the rule out; a wall's Build target on the seat's core takes
#     two ground clicks beside the commander and lands the gateway's box (`compose_wall_box`);
#     Queue a structure lists the Queue's own entries; and Undo takes every one back to the
#     committed bytes; a 300 KB paste into a field is refused before it goes, with the seat
#     connection kept up (fix pass 1), and the committed playbook is then sealed, which the
#     carried draft of round 2 checks;
#   * THE ENDED MATCH (lane `opsave`; `_ended_match_refused`), in the Probation stage before the
#     lobby's own line: a one-seat match played to its end through each recap's Continue, then
#     its line resumed through the real lobby, ends in the host's refusal ("this match has
#     ended"), the chooser back and no host left;
#   * THE HOST THAT CANNOT SAVE (lane `opsave`, fix pass 2; `_unsaved_host_said`): the real
#     lobby's new match, its host keeping its private match cache in a folder of the check's
#     own, its `save.json.partial` made a folder; Ready ends the Lull, the host answers with its
#     typed error, says why and exits, and the lobby shows the host's words (what could not be
#     written, what the disk holds) with the chooser back and the host gone;
#   * across every host: the bridge caught no panic, refused no view page, the gateway
#     refused no call (so no burst met RATE_LIMITED), entities were drawn, and every host
#     exited once the client let go of its standard input.
#
# THE MATCH ID is `watch-check-<unix seconds>-<pid>`, through host_link.gd's one helper
# (decisions-log item 113 (13)): the host keeps its save in the real per-user private match
# cache, and a later local run that reused a pid would be refused. Each local run therefore
# leaves one match folder there that the client cannot remove (godot/README.md). The check
# never touches the lobby's remembered line.
#
# The last line printed before a pass carries the hosts' process ids, so the CI step can
# also check from outside that no `gamectl` is left once Godot has exited. There is exactly
# one OK line.
#
# Run headless, after `cargo xtask stage-client` and a debug build of gamectl:
#
#     godot --headless --path godot res://scenes/watch_check.tscn -- \
#         --gamectl=<target>/debug/gamectl --root=<repository> [--segment=30000]
#
# Exit codes: 0 when everything above held, 1 when anything did not.

extends Node

const HostLink := preload("res://scripts/host_link.gd")
const Strings := preload("res://scripts/strings.gd")
const Values := preload("res://scripts/values.gd")
const LobbyScene := preload("res://scenes/lobby.tscn")
const Lobby := preload("res://scripts/lobby.gd")
## The remembered file the cut-line leg points the real lobby at: the check's own, never the
## lobby's `user://last_match.txt`.
const ENDED_REMEMBERED := "user://watch_check_last_match.txt"

## The idle stretch: longer than the gateway's 30-second READ_TIMEOUT, so a connection
## with no keep-alive would be dropped inside it.
const IDLE_WAIT := 35.0
## How long the host may take to generate its map and announce itself. A debug build
## takes about ten seconds on the owner's machine.
const START_WAIT := 180.0
## How long one editor step may take to come back.
const EDIT_WAIT := 60.0
## How long the Push may take to play out at 4x.
const PUSH_WAIT := 240.0
## How long the host may take to exit once its standard input is closed (a Lull's save is
## written first).
const EXIT_WAIT := 30.0
## The segment played: short, so the check is quick. A lobby setting, carried on the
## config line like any other.
const SEGMENT := "30000"
## Two seats: this one, and seat 1, which `gamectl host`'s built-in operator plays (Easy
## plans, submits and says ready at every Lull's start). Not one, which the brief
## recommended: a one-seat match is decided at its Push's first tick, because the last seat
## standing wins (crates/sim/src/runner.rs, `MatchState::decide`; decisions-log item 16),
## so it ends after round 1 and has no round 2 to quit in and resume. With Easy's seat
## already ready, this seat's Ready ends the Lull through `all_ready`, as the lobby's does.
const CHECK_SEATS := 2

## The golden seed, which the committed playbooks and the reference view this check
## verifies against name their vents and beacons by, so the check keeps it whatever the
## lobby hosts (the lobby's MATCH_SEED is Probation's from S3's `gwp`), and the `custom`
## preset, whose Lulls carry the countdown this check reports and drives.
const CHECK_SEED := "0x00000000ca5caded"
const CHECK_PRESET := "custom"

## The committed playbook the editor opens, in plan-core's canonical form.
const COMMITTED := "res://fixtures/editor_check.jsonc"
const OUT_OF_VOCABULARY := "res://fixtures/out_of_vocabulary.json"
const NEEDS_A_FIX := "res://fixtures/needs_a_fix.jsonc"
const EXPECTED := "res://fixtures/editor_check.expected.jsonc"
## The gateway's `instantiate_suggested` golden, copied byte for byte.
const WIZARD_FIXTURE := "res://fixtures/instantiate_suggested.json"
## The template the fixture instantiates (tests/golden/gateway/README.md says so); the one
## template this check names, to pin it.
const PINNED_TEMPLATE := "hold_and_build"
## Where the check saves the player's file, as a player would: a name other than the
## committed playbook's, so Save as... offering it after the save is the save's doing.
const SAVED := "user://editor_check_saved.jsonc"
const NOTES := "watch check"
## What the first scene hands the second (Engine metadata, which survives a scene change).
const CARRY := "pharmakos_watch_check"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink
@onready var editor: CanvasLayer = $Editor

var _failures: Array[String] = []
var _refusals: Array[String] = []
## The editor's calls the gateway refused on purpose: the planning wire's pastes past what a
## value holds (fix pass 2). Each is taken off `_refusals` as it comes, and the editor's own count
## of refusals is held to this number rather than to none.
var _asked_refusals := 0
var _events := 0
var _ended := false
var _gamectl := ""
var _root := ""
var _line := ""
var _resumed := false
## The Probation leg: the third scene, on the lobby's own line.
var _probation := false
## How long the Probation leg's Lull is watched: many clock reports' worth (four a second).
const PROBATION_WATCH := 4.0


func _ready() -> void:
	_rounds_cases()
	Engine.physics_jitter_fix = 0
	_resumed = Engine.has_meta(CARRY)
	_probation = _resumed and String((Engine.get_meta(CARRY) as Dictionary).get("stage", "")) == "probation"
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	editor.setup(vista)
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	_gamectl = HostLink.find_gamectl()
	_root = HostLink.find_root()
	var written := ""
	if _probation:
		var carried: Dictionary = Engine.get_meta(CARRY)
		_failures.assign(carried.get("failures", []))
		_refusals.assign(carried.get("refusals", []))
		_events = int(carried.get("events", 0))
		# The lobby's own line: its seed, seats and preset, as New match writes it.
		_line = HostLink.config_line(HostLink.match_id("watch-check-probation"), HostLink.LADDER)
		written = _line
		print("[watch-check] the Probation leg starts the lobby's own match: %s" % _line.strip_edges().replace("\t", " "))
	elif _resumed:
		var carried: Dictionary = Engine.get_meta(CARRY)
		_line = String(carried["line"])
		_failures.assign(carried.get("failures", []))
		_refusals.assign(carried.get("refusals", []))
		_events = int(carried.get("events", 0))
		written = HostLink.resume_line(_line)
		print("[watch-check] a fresh client resumes the match: %s" % _line.split("\t")[0])
	else:
		var ladder := SEGMENT
		for arg in OS.get_cmdline_user_args():
			if arg.begins_with("--segment="):
				ladder = arg.trim_prefix("--segment=")
		_line = HostLink.config_line(HostLink.match_id("watch-check"), ladder, CHECK_SEATS, CHECK_SEED, CHECK_PRESET)
		written = _line
		print("[watch-check] starting %s" % _gamectl)
	if _probation:
		# The Probation leg starts its own hosts: first a remembered line from before the
		# preset existed, which the gateway refuses, then the lobby's own line.
		_run_probation()
		return
	if not link.start(_gamectl, _root, written):
		_finish()
		return
	editor.begin(HostLink.my_seat())
	if not _resumed:
		# What the check asks for, held to the gateway's `get_status` (S3's `prob`).
		vista.bridge.watch_expect_preset(CHECK_PRESET)
	print("[watch-check] host pid %d" % link.host_pid())
	if _resumed:
		_run_resumed()
	else:
		_run()


## THE `--rounds` READER'S TABLE (S2's `rnds`, decisions-log item 143 (5)): host_link.gd's
## `rounds_arg` run on fixed argument lists, before anything else, so its behaviour is
## checked and not only its source. Each row is the arguments, then the `given`, `text` and
## `refusal` it must answer; any mismatch is a failure of the whole check. The value's
## meaning is the gateway's, so `0` and an empty `--rounds=` pass through as text.
func _rounds_cases() -> void:
	var none := HostLink.RoundsRefusal.NONE
	var no_value := HostLink.RoundsRefusal.NO_VALUE
	var repeated := HostLink.RoundsRefusal.REPEATED
	var breaks := HostLink.RoundsRefusal.BREAKS_LINE
	var cases := [
		[[], false, "", none],
		[["--gamectl=x", "--root=y"], false, "", none],
		[["--rounds", "6"], true, "6", none],
		[["--rounds=6"], true, "6", none],
		[["--rounds", "0"], true, "0", none],
		[["--rounds="], true, "", none],
		[["--rounds"], true, "", no_value],
		[["--rounds", "--gamectl=x"], true, "", no_value],
		[["--rounds=4", "--rounds=4"], true, "", repeated],
		[["--rounds", "4", "--rounds", "5"], true, "", repeated],
		[["--rounds", "--rounds=4"], true, "", repeated],
		[["--rounds=4\t5"], true, "", breaks],
		[["--rounds", "4\n"], true, "", breaks],
		[["--rounds=4\r"], true, "", breaks],
	]
	for case in cases:
		var args := PackedStringArray(case[0])
		var got: Dictionary = HostLink.rounds_arg(args)
		var want := {"given": case[1], "text": case[2], "refusal": case[3]}
		if got["given"] != want["given"] or String(got["text"]) != String(want["text"]) or int(got["refusal"]) != int(want["refusal"]):
			_failures.append("rounds_arg(%s) answered %s, not %s" % [JSON.stringify(case[0]), JSON.stringify(got), JSON.stringify(want)])
	print("[watch-check] the --rounds reader's table: %d case(s)" % cases.size())
	_option_cases()


## The `--gamectl=` and `--root=` reader's table (S2's `view`; decisions-log item 146 (4)):
## absent, once, twice, and an option that only starts the same, run through
## `host_link.gd`'s `prefixed_arg` before anything else, as the `--rounds` table is.
func _option_cases() -> void:
	var cases := [
		[[], "--gamectl=", false, "", false],
		[["--gamectl=x"], "--gamectl=", true, "x", false],
		[["--gamectl="], "--gamectl=", true, "", false],
		[["--gamectl=x", "--root=y"], "--root=", true, "y", false],
		[["--gamectl=x", "--gamectl=x"], "--gamectl=", true, "", true],
		[["--root=a", "--rounds=3", "--root=b"], "--root=", true, "", true],
		[["--gamectlx"], "--gamectl=", false, "", false],
	]
	for case in cases:
		var got: Dictionary = HostLink.prefixed_arg(PackedStringArray(case[0]), String(case[1]))
		var want := {"given": case[2], "text": case[3], "repeated": case[4]}
		if got["given"] != want["given"] or String(got["text"]) != String(want["text"]) or got["repeated"] != want["repeated"]:
			_failures.append("prefixed_arg(%s, %s) answered %s, not %s" % [JSON.stringify(case[0]), case[1], JSON.stringify(got), JSON.stringify(want)])
	print("[watch-check] the --gamectl= and --root= reader's table: %d case(s)" % cases.size())


func _process(_delta: float) -> void:
	if not _ended:
		link.pump()


func _run() -> void:
	_playtest_on()
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and _state().get("view_settled", false), START_WAIT):
		_failures.append("the host never reached a Lull with a complete keyframe")
		_finish()
		return
	print("[watch-check] Lull reached; %d markers drawn" % vista.marker_count())
	if not await _until(func() -> bool: return _state().get("preset_check", "") != "not_read", EDIT_WAIT) or _state().get("preset_check", "") != "agrees" or _state().get("preset", "") != CHECK_PRESET:
		_failures.append("get_status does not name the %s match the check asked for: %s" % [CHECK_PRESET, _state()])
		_finish()
		return
	if not await _until(func() -> bool: return _state().has("sealed_this_round"), EDIT_WAIT):
		_failures.append("the seat's get_status did not tell it whether it holds a seal: %s" % _state())
		_finish()
		return

	if not await _edit():
		_finish()
		return
	if not await _meter("lull"):
		_finish()
		return
	if not await _templates():
		_finish()
		return
	if not await _panel_lines():
		_finish()
		return
	if not await _pin():
		_finish()
		return
	if not await _camera_keys():
		_finish()
		return
	if not await _first_click_lands():
		_finish()
		return
	if not await _map_click_opens_a_menu():
		_finish()
		return
	if not await _real_wheel():
		_finish()
		return
	if not await _planning_wire():
		_finish()
		return
	var sealed: PackedByteArray = vista.bridge.editor_sealed_bytes()
	print("[watch-check] round 1's sealed submission is %d bytes; the Lull's timer shows %s at Ready" % [sealed.size(), _state().get("timer", "")])

	vista.bridge.watch_command("speed", 4)
	vista.bridge.watch_command("ready", 0)
	if not await _until(func() -> bool: return _state().get("phase", "") == "push", START_WAIT):
		_failures.append("Ready did not end the Lull")
		_finish()
		return
	print("[watch-check] Ready ended the Lull; Push begun at 4x")
	if not await _refused_in_the_push():
		_finish()
		return
	vista.bridge.playtest_focus(false)
	var follow: Node3D = vista.my_commander()
	if follow != null:
		vista.rig.follow(follow)
	if not await _meter("push"):
		_finish()
		return
	vista.bridge.playtest_focus(true)
	if not await _until(func() -> bool: return _state().get("phase", "") == "recap", PUSH_WAIT):
		_failures.append("the Push never reached its recap")
		_finish()
		return
	if not await _until(func() -> bool: return _state().get("seat_settled", false), START_WAIT):
		_failures.append("the seat connection never caught up after the recap began")
		_finish()
		return
	# The recap's lines are get_recap's prose, read once the recap opened (S1's plan, `ui`).
	if not await _until(func() -> bool: return int(vista.bridge.watch_recap().get("answers", 0)) > 0, EDIT_WAIT):
		_failures.append("the recap was never read: %s" % vista.bridge.watch_recap())
		_finish()
		return
	var recap: Dictionary = vista.bridge.watch_recap()
	if String(recap.get("prose", "")) == "" or int(recap.get("round", 0)) != 1:
		_failures.append("the recap is not round 1's prose: %s" % recap)
		_finish()
		return
	# Read once, not in a loop: the one answer so far is the only one asked for.
	if _count(recap, "answers") != 1:
		_failures.append("the recap was read more than once: %s" % recap)
		_finish()
		return
	print("[watch-check] recap: %s" % recap.get("prose", ""))
	if not _playtest_bundle():
		_finish()
		return

	# Round 2: Continue, and the carried draft opens through get_draft.
	vista.bridge.watch_command("end_recap", 0)
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and int(_state().get("round", 0)) == 2, START_WAIT):
		_failures.append("Continue did not lead into round 2's Lull")
		_finish()
		return
	if not await _carried(sealed, "round 2"):
		_finish()
		return
	print("[watch-check] round 2: the carried draft opened through get_draft, round 1's sealed bytes")

	# The idle stretch, in round 2's Lull: no connection may drop across it.
	print("[watch-check] idling %.0f s" % IDLE_WAIT)
	await get_tree().create_timer(IDLE_WAIT).timeout
	var idle := _state()
	print("[watch-check] after the idle stretch: %s" % idle)
	# A close on a message (the planning wire's six big loads, fix pass 4) is not a dropped
	# connection: what may not happen is a drop of the connection itself.
	if idle.get("phase", "") != "lull" or _connection_drops(idle, "admin") != 0 or _connection_drops(idle, "seat") != 0:
		_failures.append("the idle stretch did not stay a Lull with both connections up: %s" % idle)
		_finish()
		return

	# Quit in the Lull: the host saves it and exits once its standard input closes.
	var pid: int = link.host_pid()
	_ended = true
	link.quit()
	if not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
		_failures.append("the first host (pid %d) was still running after its standard input closed" % pid)
	_check_bridge("the first client")
	if not _failures.is_empty():
		_finish()
		return
	print("[watch-check] the first host exited once its standard input closed; changing scene")
	Engine.set_meta(CARRY, {
		"line": _line,
		"sealed": sealed,
		"failures": _failures,
		"refusals": _refusals,
		"events": _events,
		"first_pid": pid,
	})
	get_tree().change_scene_to_file("res://scenes/watch_check.tscn")


## The fresh client's half: round 2's Lull, the notebook, the drafts and the carried draft.
func _run_resumed() -> void:
	var carried: Dictionary = Engine.get_meta(CARRY)
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and _state().get("view_settled", false), START_WAIT):
		_failures.append("the resumed host never reached a Lull with a complete keyframe")
		_finish()
		return
	if int(_state().get("round", 0)) != 2:
		_failures.append("the match resumed into round %s, not round 2's Lull" % _state().get("round"))
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("notes_known", false) and s.get("drafts_known", false), "the resumed notebook and drafts were never read"):
		_finish()
		return
	if String(_editor().get("notes", "")) != NOTES:
		_failures.append("the notebook did not come back: `%s`" % _editor().get("notes"))
	if not _has_draft(_editor(), "carried"):
		_failures.append("the carried draft is not listed after the resume: %s" % [_editor().get("drafts")])
	if not _has_draft(_editor(), "editor"):
		_failures.append("the editor's own draft is not listed after the resume: %s" % [_editor().get("drafts")])
	await _carried(carried["sealed"], "the resumed Lull")
	print("[watch-check] resumed into round %s's Lull: notes back, %d draft(s) listed, the carried draft opened through get_draft" % [_state().get("round"), (_editor().get("drafts", []) as Array).size()])
	_finish()


## The Probation leg: the lobby's own match reaches its first Lull, whose footer says it is
## untimed by rule; across a stretch of clock reports the rig sends no countdown (the gateway
## would refuse one), the footer stays untimed, and no countdown is shown.
func _run_probation() -> void:
	if not await _older_line_refused():
		_finish()
		return
	if not await _cut_line_refused():
		_finish()
		return
	if not await _ended_match_refused():
		_finish()
		return
	if not await _unsaved_host_said():
		_finish()
		return
	if not link.start(_gamectl, _root, _line):
		_finish()
		return
	editor.begin(HostLink.my_seat())
	vista.bridge.watch_expect_preset(HostLink.PRESET)
	print("[watch-check] host pid %d" % link.host_pid())
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and _state().get("view_settled", false), START_WAIT):
		_failures.append("the Probation host never reached a Lull with a complete keyframe")
		_finish()
		return
	if not await _probation_frame():
		_finish()
		return
	var refused_before := _refusals.size()
	var before := _state()
	await get_tree().create_timer(PROBATION_WATCH).timeout
	var state := _state()
	# Asked, not merely silent: clock reports went out and were answered, none with a countdown.
	var answered := int(state.get("clock_answers", 0)) - int(before.get("clock_answers", 0))
	var countdowns := int(state.get("countdowns_sent", 0)) - int(before.get("countdowns_sent", 0))
	if answered < 1:
		_failures.append("the Probation Lull: no clock report was answered in %.0f s, so nothing was asked" % PROBATION_WATCH)
	if countdowns != 0:
		_failures.append("the Probation Lull: %d clock report(s) carried a countdown" % countdowns)
	if not state.get("untimed_by_rule", false):
		_failures.append("the Probation Lull's footer does not say untimed by rule: %s" % state)
	if _refusals.size() != refused_before:
		_failures.append("the Probation Lull: the gateway refused %s (a countdown report would be INVALID_ARGUMENT)" % "; ".join(_refusals.slice(refused_before)))
	if not state.get("untimed", false):
		_failures.append("the Probation Lull's footer stopped saying untimed: %s" % state)
	if String(state.get("timer", "")) != "":
		_failures.append("the Probation Lull shows a countdown: `%s`" % state.get("timer"))
	if state.get("phase", "") != "lull":
		_failures.append("the Probation Lull ended by itself: %s" % state)
	print("[watch-check] Probation: %.0f s of the Lull, %d clock report(s) answered, %d countdown(s) sent, none shown, untimed by rule %s, %d refusal(s)" % [PROBATION_WATCH, answered, countdowns, state.get("untimed_by_rule"), _refusals.size() - refused_before])
	await _host_gone()
	_finish()


## **A host that truly goes away ends the link** (fix pass 4, item 1): the host is killed, and
## the link gives up in words rather than reconnecting for ever.
func _host_gone() -> void:
	var pid: int = link.host_pid()
	# Up to here no connection may have dropped; the drops the host's going makes are this leg's.
	if _connection_drops(_state(), "admin") != 0 or _connection_drops(_state(), "seat") != 0:
		_failures.append("a connection dropped before the host went away: %s" % _state())
	_host_killed = true
	_refused_reason = ""
	_expecting_refusal = true
	if OS.kill(pid) != OK:
		_failures.append("the host-gone leg could not end the host (pid %d)" % pid)
		_expecting_refusal = false
		return
	var ended := await _until(func() -> bool: return _refused_reason != "", START_WAIT)
	_expecting_refusal = false
	if not ended:
		_failures.append("the host went away and the link never gave up")
		return
	# The give-up is the reconnect policy's, in its own words (fix pass 5).
	if not _refused_reason.contains("connection was given up after") or not Strings.text("link_gave_up").contains("connection was given up after"):
		_failures.append("the host went away and the link gave up in words that are not the policy's: %s" % _refused_reason)
	# Never a bare -1 (fix pass 6): a close with no code is said as a lost connection.
	if _refused_reason.contains("-1"):
		_failures.append("the give-up says a bare -1: %s" % _refused_reason)
	var lost := HostLink.gave_up_words(1, {"attempts": 4, "code": -1})
	var coded := HostLink.gave_up_words(0, {"attempts": 4, "code": 1006})
	if lost.contains("-1") or not lost.contains("lost with no close") or not coded.contains("code 1006"):
		_failures.append("the give-up's words by code are wrong: %s | %s" % [lost, coded])
	print("[watch-check] the host went away (pid %d) and the reconnect policy gave the link up: %s" % [pid, _refused_reason])

## How long the silent-host leg may take to give the link up: six connects aborted at the connect
## deadline (5 s) and the back-off between them (17.5 s), with room for a slow frame.
const SILENT_WAIT := 90.0

## **A host that accepts a connection and never answers is given up in words** (fix pass 7,
## review 6's TCPServer probe): a listener takes every connection and never reads or writes it,
## so the handshake never completes. The links are attached to it with no host process; the rig
## has each connect aborted at its deadline, and the policy gives the link up within
## `SILENT_WAIT`, in the policy's words. Before the fix the peer stayed connecting for ever.
func _silent_host() -> void:
	var server := TCPServer.new()
	if server.listen(0, "127.0.0.1") != OK:
		_failures.append("the silent-host leg could not listen")
		return
	var held: Array[StreamPeerTCP] = []
	link.reset()
	_refused_reason = ""
	_expecting_refusal = true
	link.attach(server.get_local_port(), ["silent-admin", "silent-seat"])
	var timer := get_tree().create_timer(SILENT_WAIT)
	while _refused_reason == "" and timer.time_left > 0.0:
		while server.is_connection_available():
			held.append(server.take_connection())
		link.pump_attached()
		await get_tree().process_frame
	_expecting_refusal = false
	var left := timer.time_left
	link.reset()
	server.stop()
	if held.is_empty():
		_failures.append("the silent-host leg: the listener took no connection")
	if _refused_reason == "":
		_failures.append("the silent-host leg: a host that never answered was not given up in %.0f s" % SILENT_WAIT)
		return
	if not _refused_reason.contains("connection was given up after"):
		_failures.append("the silent-host leg gave up in words that are not the policy's: %s" % _refused_reason)
	print("[watch-check] a host that accepts and never answers: %d connection(s) taken, given up with %.1f s of the %.0f s bound left: %s" % [held.size(), left, SILENT_WAIT, _refused_reason])
	await _silent_after_open()


## How long the open-then-silent leg waits for its first abort: the call deadline (20 s) and a
## slow frame's room.
const DEADLINE_WAIT := 40.0
## How long an aborted peer may take to read CLOSED (measured: 32 ms).
const ABORT_WAIT := 2.0

## **A host that opens and then never answers is aborted, not closed by a handshake** (fix pass
## 7, item 1 (a)): a listener completes each handshake and then never polls again. The call in
## flight passes its deadline, the rig has the peer aborted, and the aborted peer reads CLOSED
## within `ABORT_WAIT`. A `close(1000)` would wait for an echo that never comes, and the peer
## would read CLOSING for ever (review 6 measured 40 s).
func _silent_after_open() -> void:
	var server := TCPServer.new()
	if server.listen(0, "127.0.0.1") != OK:
		_failures.append("the open-then-silent leg could not listen")
		return
	var answering: Array[WebSocketPeer] = []
	link.reset()
	_refused_reason = ""
	_expecting_refusal = true
	link.attach(server.get_local_port(), ["silent-admin", "silent-seat"])
	var first: WebSocketPeer = link._peers[0]
	var timer := get_tree().create_timer(DEADLINE_WAIT)
	while link._peers[0] == first and _refused_reason == "" and timer.time_left > 0.0:
		while server.is_connection_available():
			var peer := WebSocketPeer.new()
			peer.accept_stream(server.take_connection())
			answering.append(peer)
		# Each server peer is polled until its handshake is done, and never after.
		for peer in answering:
			if peer.get_ready_state() == WebSocketPeer.STATE_CONNECTING:
				peer.poll()
		link.pump_attached()
		await get_tree().process_frame
	_expecting_refusal = false
	var aborted: bool = link._peers[0] != first
	var closed := false
	if aborted:
		var settle := get_tree().create_timer(ABORT_WAIT)
		while settle.time_left > 0.0 and not closed:
			first.poll()
			closed = first.get_ready_state() == WebSocketPeer.STATE_CLOSED
			await get_tree().process_frame
	var state := first.get_ready_state()
	link.reset()
	server.stop()
	if not aborted:
		_failures.append("the open-then-silent leg: the admin link was not aborted in %.0f s (%s)" % [DEADLINE_WAIT, _refused_reason])
		return
	if not closed:
		_failures.append("the open-then-silent leg: the aborted peer still reads state %d after %.0f s, not CLOSED" % [state, ABORT_WAIT])
		return
	print("[watch-check] a host that opens and never answers: the call deadline aborted the admin link, and the aborted peer read CLOSED; the link was opened again")


## A line remembered before the preset existed (six fields) is refused by the gateway when it
## is resumed, and the link then starts another host (S3's `prob`; the plan's E6): what the
## lobby's refused Resume leans on. The refusal's words are printed, never read: any refusal
## before an announce is the refusal this needs. The refused host is gone once it refused.
var _refused_reason := ""
## Whether the host-gone leg ended the host: its drops are the leg's, checked before it.
var _host_killed := false
var _expecting_refusal := false


func _older_line_refused() -> bool:
	var fields := HostLink.config_line(HostLink.match_id("watch-check-older"), HostLink.LADDER).strip_edges().split("\t")
	fields.resize(6)
	_expecting_refusal = true
	if not link.start(_gamectl, _root, HostLink.resume_line("\t".join(fields))):
		_expecting_refusal = false
		return false
	var refused_pid: int = link.host_pid()
	var refused := await _until(func() -> bool: return _refused_reason != "", START_WAIT)
	_expecting_refusal = false
	if not refused:
		_failures.append("a six-field remembered line was not refused when resumed")
		return false
	if OS.is_process_running(refused_pid):
		_failures.append("the host that refused the older line (pid %d) is still running" % refused_pid)
		return false
	link.reset()
	print("[watch-check] a six-field remembered line was refused on resume: %s" % _refused_reason)
	return true


## **A remembered line cut mid-field is the host's to refuse** (S3's `fixq-c`, fix pass 3,
## item 1b: what the lobby's reader leans on, as the six-field line above is). The real lobby,
## pointed at a file of the check's own holding a line cut inside its second field, offers it;
## Resume ends in the host's refusal in its own words, with the chooser back and no match
## started, never a crash or a hang. The line is not parsed here: the host reads it.
func _cut_line_refused() -> bool:
	var cut_from := HostLink.config_line(HostLink.match_id("watch-check-cut"), SEGMENT, 1, CHECK_SEED, CHECK_PRESET).strip_edges()
	if not await _resume_refused("a line cut mid-field", cut_from.get_slice("\t", 0) + "\t" + cut_from.get_slice("\t", 1).left(4)):
		return false
	var path := ProjectSettings.globalize_path(ENDED_REMEMBERED)
	if FileAccess.file_exists(path) and DirAccess.remove_absolute(path) != OK:
		_failures.append("the cut-line leg could not remove %s" % path)
		return false
	return true


## The real lobby, offered `line` from the check's own remembered file: Resume is shown, and
## pressing it ends in the host's refusal (`lobby_resumed_failed`), the chooser back, no match
## started and no host left running. `words`, when given, is what the host's refusal says.
func _resume_refused(name: String, line: String, words: String = "") -> bool:
	var file := FileAccess.open(ENDED_REMEMBERED, FileAccess.WRITE)
	if file == null:
		_failures.append("the cut-line leg could not open %s (error %d)" % [ENDED_REMEMBERED, FileAccess.get_open_error()])
		return false
	var stored := file.store_string(line + "\n")
	file.close()
	if not stored:
		_failures.append("the cut-line leg could not write %s" % ENDED_REMEMBERED)
		return false
	var lobby: Node = LobbyScene.instantiate()
	lobby.remembered = ENDED_REMEMBERED
	add_child(lobby)
	await get_tree().process_frame
	var offered: bool = lobby._resume_button.visible
	if not offered:
		_failures.append("the lobby does not offer %s" % name)
	else:
		lobby.resume_match()
		var settled := await _until(func() -> bool: return lobby._announced or (not lobby._started and lobby._chooser.visible), START_WAIT)
		var refused_prefix := Strings.text("lobby_resumed_failed", {"reason": ""}).get_slice(":", 0)
		if lobby._announced:
			_failures.append("STOP: the host resumed %s instead of refusing it: %s" % [name, lobby._status.text])
		elif not settled:
			_failures.append("Resume of %s neither was refused nor resumed: `%s`" % [name, lobby._status.text])
		elif not String(lobby._status.text).begins_with(refused_prefix):
			_failures.append("Resume of %s did not end in the host's refusal: `%s`" % [name, lobby._status.text])
		elif words != "" and not String(lobby._status.text).contains(words):
			_failures.append("Resume of %s was refused in other words than `%s`: `%s`" % [name, words, lobby._status.text])
		else:
			print("[watch-check] Resume of %s was refused by the host: %s" % [name, String(lobby._status.text).strip_edges().replace("\n", " ")])
		var pid: int = lobby.link.host_pid()
		lobby.link.quit()
		if pid >= 0 and not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
			_failures.append("the host that refused %s (pid %d) was still running" % [name, pid])
	lobby.queue_free()
	await get_tree().process_frame
	return _failures.is_empty()


## The Probation match's frame on a real gateway (S3's `prob`): `get_status` names it
## `probation`, as New match asked; round 1's first `list_templates` marks exactly one
## template with a sentence, and the panel draws the mark and the sentence over it.
func _probation_frame() -> bool:
	if not await _until(func() -> bool: return _state().get("preset_check", "") != "not_read", EDIT_WAIT):
		_failures.append("the Probation match's get_status was never read: %s" % _state())
		return false
	if _state().get("preset_check", "") != "agrees" or _state().get("preset", "") != "probation":
		_failures.append("get_status does not name the lobby's Probation match: %s" % _state())
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("templates_known", false), "the Probation Lull's template list was never read"):
		return false
	var marked: Array = []
	for template in _editor().get("templates", []):
		if template.get("guided", false):
			marked.append(template)
	if marked.size() != 1 or String(marked[0].get("guided_reason", "")) == "":
		_failures.append("round 1's template list does not mark one template with a sentence: %s" % [_editor().get("templates")])
		return false
	editor.refresh()
	var drawn: Array = editor.guided_drawn()
	if drawn != [String(marked[0].get("id", "")), String(marked[0].get("guided_reason", ""))]:
		_failures.append("the panel does not draw the guided mark as sent: %s against %s" % [drawn, marked[0]])
		return false
	print("[watch-check] Probation: get_status names `probation`; round 1 marks %s: %s" % [marked[0].get("id"), marked[0].get("guided_reason")])
	return true


## The carried draft is opened (through `get_draft`, the editor's one way to open it), with
## `sealed`'s bytes.
func _carried(sealed: PackedByteArray, where: String) -> bool:
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("status_key", "")) == "carried" and not s.get("busy", true) and s.get("rows_current", false), "%s: the carried draft was never opened" % where):
		return false
	var opened: PackedByteArray = vista.bridge.editor_bytes()
	if opened != sealed:
		_failures.append("%s: the carried draft is not round 1's sealed submission (%d bytes against %d)" % [where, opened.size(), sealed.size()])
		return false
	if int(_editor().get("refusals", 0)) != _asked_refusals:
		_failures.append("%s: the gateway refused %s of the editor's calls (%d asked for)" % [where, _editor().get("refusals"), _asked_refusals])
	return await _save_as_offers("", "%s: the carried draft" % where)


## The meter is drawn from an answer read in `phase`, and its text is strings.gd's frame over
## that answer's four fields, exactly as they came.
func _meter(phase: String) -> bool:
	if not await _until(func() -> bool: return int(vista.bridge.watch_meter().get("answers", 0)) > 0 and vista.bridge.watch_meter().get("phase", "") == phase, EDIT_WAIT):
		_failures.append("the meter was never read in the %s: %s" % [phase, vista.bridge.watch_meter()])
		return false
	editor.refresh()
	var meter: Dictionary = vista.bridge.watch_meter()
	var expected := Strings.text("meter", {"treasury": meter.get("treasury_now"), "supply": meter.get("supply_kw_now"), "draw": meter.get("draw_kw_now"), "headroom": meter.get("headroom_kw_now")})
	if editor.meter_text() != expected:
		_failures.append("the meter shows `%s`, not the answer's `%s`" % [editor.meter_text(), expected])
		return false
	print("[watch-check] meter in the %s: %s" % [phase, editor.meter_text()])
	return true


## The wizard, for every template `list_templates` lists.
func _templates() -> bool:
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("templates_known", false), "the template list was never read"):
		return false
	var templates: Array = _editor().get("templates", [])
	if templates.is_empty():
		_failures.append("list_templates listed no template")
		return false
	editor.refresh()
	if editor.template_buttons().size() != templates.size():
		_failures.append("%d template buttons for %d templates" % [editor.template_buttons().size(), templates.size()])
	for template in templates:
		if not await _template(String(template.get("id", ""))):
			return false
	return true


## One template's leg: the pages, one explicit re-send, Use, a map action, QUICK and FULL,
## the rule list, submit, Save and `gamectl verify`.
func _template(id: String) -> bool:
	editor.open_template(id)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("template_id", "") == id and s.get("wizard", {}).get("current", false), "%s: the wizard never answered" % id):
		return false
	var wizard: Dictionary = _editor()["wizard"]
	if not _pages_drawn_as_they_came(id, wizard):
		return false
	var pages: Array = wizard.get("pages", [])
	print("[watch-check] %s: %d page(s): %s; why: %s" % [id, pages.size(), _describe(pages), wizard.get("why", "")])

	# One page's shown value, sent back explicitly through the page's own Send button: its
	# mark goes and it shows what was sent.
	var first: Control = editor.wizard_page_controls()[0]
	var pointer := String(first.get_meta("pointer"))
	var shown: String = (first.get_meta("field") as LineEdit).text
	# The pages the gateway marked as the operator's, other than the one re-sent: they are
	# not named in the request, so they keep their suggestion - mark and value - through it.
	var kept := {}
	for page in pages:
		if String(page.get("pointer", "")) != pointer and bool(page.get("suggested", false)):
			kept[String(page.get("pointer", ""))] = String(page.get("value", ""))
	(first.get_meta("send") as Button).pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("current", false) and not bool(_page_of(s.get("wizard", {}), pointer).get("suggested", true)), "%s: the explicit value never came back" % id):
		return false
	editor.refresh()
	var resent: Control = editor.wizard_page_controls()[0]
	if (resent.get_meta("mark") as Label).visible:
		_failures.append("%s: the re-sent page still carries the operator's mark" % id)
	if (resent.get_meta("field") as LineEdit).text != shown:
		_failures.append("%s: the re-sent page shows `%s`, not what was sent, `%s`" % [id, (resent.get_meta("field") as LineEdit).text, shown])
	var after: Dictionary = _editor()["wizard"]
	for kept_pointer in kept:
		var page := _page_of(after, String(kept_pointer))
		if not bool(page.get("suggested", false)) or String(page.get("value", "")) != String(kept[kept_pointer]):
			_failures.append("%s: an untouched page lost its suggestion when another was re-sent: %s" % [id, kept_pointer])
	print("[watch-check] %s: %d untouched suggested page(s) kept their suggestion through the re-send" % [id, kept.size()])

	# Use, through its own button: the gateway's playbook, byte for byte, as one edit.
	var text := String(_editor()["wizard"].get("playbook_jsonc", ""))
	var before := _count(_editor(), "revision")
	editor.refresh()
	var use: Button = editor.wizard_use_button()
	if use == null or use.disabled:
		_failures.append("%s: Use is not offered on a current wizard" % id)
		return false
	use.pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and not s.get("busy", true), "%s: Use never put the playbook in" % id):
		return false
	if vista.bridge.editor_bytes() != text.to_utf8_buffer():
		_failures.append("%s: the editor's text is not the gateway's playbook_jsonc byte for byte" % id)
		return false
	if not await _save_as_offers("", "%s: the template's playbook" % id):
		return false

	# One map action through patch_plan: Alt-click the seat's own beacon, Go here.
	var own := {}
	for beacon in vista.beacons():
		if beacon["owner"] == vista.my_seat:
			own = beacon
	if own.is_empty():
		_failures.append("the view shows no beacon of this seat's")
		return false
	var used := _count(_editor(), "revision")
	editor._on_beacon_clicked(own, true, Vector2.ZERO)
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > used and s.get("verdict", "") == "quick" and s.get("rows_current", false), "%s: the map action was never patched and checked" % id):
		return false
	if not _editor().get("qualifies", false):
		_failures.append("%s: QUICK did not qualify the edited playbook: %s" % [id, _editor().get("rows", [])])
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("verdict", "") == "full" and s.get("rows_current", false) and s.get("prose_current", false), "%s: FULL or the rule list never came" % id):
		return false
	if not _editor().get("qualifies", false):
		_failures.append("%s: FULL did not qualify the edited playbook: %s" % [id, _editor().get("rows", [])])
	_check_rule_names(id)

	# Submit: accepted, and the bytes that went are the gateway's last text.
	var edited: PackedByteArray = vista.bridge.editor_bytes()
	editor.submit()
	if not await _editor_until(func(s: Dictionary) -> bool: return not s.get("pending", true) and vista.bridge.editor_sealed_bytes() == edited, "%s: the submission never came back accepted" % id):
		_failures.append("%s: submission rows %s" % [id, _editor().get("rows", [])])
		return false

	# Save, and gamectl verify against its reference seat view.
	var saved := "user://watch_check_%s.jsonc" % id
	if not editor.save_file(saved):
		_failures.append("%s: the submitted playbook could not be saved" % id)
		return false
	_verify(id, saved)
	bridge_close_wizard()
	return true


## Every page the answer lists is drawn, in order, as it came; the why too.
func _pages_drawn_as_they_came(id: String, wizard: Dictionary) -> bool:
	editor.refresh()
	var pages: Array = wizard.get("pages", [])
	var drawn: Array[Control] = editor.wizard_page_controls()
	if pages.is_empty():
		_failures.append("%s: the answer lists no parameter" % id)
		return false
	if drawn.size() != pages.size():
		_failures.append("%s: %d page(s) drawn for %d parameter(s)" % [id, drawn.size(), pages.size()])
		return false
	for index in pages.size():
		var page: Dictionary = pages[index]
		var control: Control = drawn[index]
		var field: LineEdit = control.get_meta("field")
		var mark: Label = control.get_meta("mark")
		if String(control.get_meta("pointer")) != String(page.get("pointer", "")):
			_failures.append("%s: page %d is out of order" % [id, index])
		if control.accessibility_name != String(page.get("label", "")) or String(control.get_meta("label")) != String(page.get("label", "")):
			_failures.append("%s: page %d's label is not the gateway's `%s`" % [id, index, page.get("label")])
		if field.text != String(page.get("value", "")):
			_failures.append("%s: page %d shows `%s`, not the raw value `%s`" % [id, index, field.text, page.get("value")])
		if mark.visible != bool(page.get("suggested", false)):
			_failures.append("%s: page %d's mark is %s where the gateway says %s" % [id, index, mark.visible, page.get("suggested")])
	var why := String(wizard.get("why", ""))
	var expected := Strings.text("wizard_why", {"why": why}) if why != "" else ""
	if editor.wizard_why_text() != expected:
		_failures.append("%s: the why shows `%s`, not `%s`" % [id, editor.wizard_why_text(), expected])
	return _failures.is_empty()


## `gamectl verify` on the saved file. It checks against the reference seat view, not this
## match, so its only errors may be the three that view makes of this match's names and
## places: E0403 at a placed site, E0412 at a named feature and E0401 at a named beacon
## (the live FULL at submit is that file's check).
func _verify(id: String, saved: String) -> void:
	var output := []
	var code := OS.execute(_gamectl, PackedStringArray(["--root", _root, "verify", ProjectSettings.globalize_path(saved)]), output, true)
	var report := "".join(output)
	var errors: PackedStringArray = []
	for line in report.split("\n"):
		var words := line.strip_edges().split(" ", false)
		if words.size() >= 3 and words[0] == "error":
			errors.append("%s %s" % [words[1], words[2]])
	for error in errors:
		var placed := error.begins_with("E0403 ") and "/place_beacon/" in error
		var feature := error.begins_with("E0412 ") and error.ends_with("/feature_id")
		var beacon := error.begins_with("E0401 ") and error.ends_with("/beacon_id")
		if not (placed or feature or beacon):
			_failures.append("%s: gamectl verify found more than the reference view's E0403, E0412 and E0401: %s" % [id, error])
	if code != 0 and errors.is_empty():
		_failures.append("%s: gamectl verify failed with no error listed (exit %d): %s" % [id, code, report])
	print("[watch-check] %s: submitted and accepted; the bytes are the gateway's last text; gamectl verify exit %d%s" % [id, code, (" (errors: %s)" % ", ".join(errors)) if not errors.is_empty() else ""])


## What the panel says after Undo and after Load (see the header). Returns false when a step
## failed, with the reason recorded.
func _panel_lines() -> bool:
	editor.open_template(PINNED_TEMPLATE)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("template_id", "") == PINNED_TEMPLATE and s.get("wizard", {}).get("current", false), "the panel lines: the template never answered"):
		return false
	var depth := _count(_editor(), "undo_depth")
	var used := _count(_editor(), "revision")
	vista.bridge.editor_wizard_use()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > used and not s.get("busy", true) and not s.get("pending", true), "the panel lines: Use never put the playbook in"):
		return false
	editor.refresh()
	if editor.status_text() != Strings.text("status_wizard_used"):
		_failures.append("after Use the status line says `%s`" % editor.status_text())
	var unused := _count(_editor(), "revision")
	vista.bridge.editor_undo()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > unused and _count(s, "undo_depth") == depth and not s.get("busy", true), "the panel lines: Undo never came back"):
		return false
	editor.refresh()
	if editor.status_text() != Strings.text("status_undone"):
		_failures.append("after Undo the status line says `%s`, not that it was undone" % editor.status_text())
	print("[watch-check] Undo of a template's playbook says: %s" % editor.status_text())
	if not _editor().get("wizard", {}).is_empty():
		editor.load_file(COMMITTED)
	else:
		_failures.append("Undo closed the wizard")
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("status_key", "")) == "loaded" and s.get("rows_current", false) and not s.get("busy", true), "the panel lines: the committed playbook never opened"):
		return false
	editor.refresh()
	if not _editor().get("wizard", {}).is_empty() or not editor.wizard_page_controls().is_empty():
		_failures.append("the wizard stayed open after Load opened another file")
	var size: Dictionary = _editor().get("size", {})
	var counted := Strings.text("size_meter", {"units": size.get("units"), "budget": size.get("budget")})
	if size.is_empty() or int(size.get("budget", 0)) <= 0 or editor.size_text() != counted:
		_failures.append("the size meter says `%s` for the report's %s" % [editor.size_text(), size])
	print("[watch-check] Load closed the wizard; the size meter says: %s" % editor.size_text())
	return true


## Hold & Build with every parameter the fixture lists typed explicitly, read from the
## fixture: the instantiated text is the fixture's `playbook_jsonc`, byte for byte. Only
## instantiated; never used or submitted.
func _pin() -> bool:
	var fixture: Dictionary = vista.bridge.editor_wizard_of_response(FileAccess.get_file_as_string(WIZARD_FIXTURE))
	if fixture.is_empty():
		_failures.append("%s did not decode" % WIZARD_FIXTURE)
		return false
	editor.open_template(PINNED_TEMPLATE)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("template_id", "") == PINNED_TEMPLATE and s.get("wizard", {}).get("current", false), "the pinned template never answered"):
		return false
	# Typed into each page's own field and sent with its own Send button, as a player would.
	editor.refresh()
	for page in fixture.get("pages", []):
		var typed := false
		for control in editor.wizard_page_controls():
			if String(control.get_meta("pointer")) == String(page.get("pointer", "")):
				(control.get_meta("field") as LineEdit).text = String(page.get("value", ""))
				(control.get_meta("send") as Button).pressed.emit()
				typed = true
		if not typed:
			_failures.append("the pinned template has no page at the fixture's `%s`" % page.get("pointer"))
			return false
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("current", false) and _all_explicit(s.get("wizard", {})), "the pinned template's explicit values never came back"):
		return false
	var made := String(_editor()["wizard"].get("playbook_jsonc", ""))
	var pinned := String(fixture.get("playbook_jsonc", ""))
	if made != pinned:
		_failures.append("the pinned template with the fixture's values typed explicitly is not the fixture's playbook_jsonc byte for byte (%d bytes against %d)" % [made.to_utf8_buffer().size(), pinned.to_utf8_buffer().size()])
		printerr("[watch-check] instantiated:\n%s" % made)
		printerr("[watch-check] fixture:\n%s" % pinned)
		return false
	print("[watch-check] %s with the fixture's %d values typed explicitly is the fixture's playbook_jsonc byte for byte (instantiated only)" % [PINNED_TEMPLATE, (fixture.get("pages", []) as Array).size()])
	bridge_close_wizard()
	return true


func bridge_close_wizard() -> void:
	vista.bridge.editor_wizard_close()


func _all_explicit(wizard: Dictionary) -> bool:
	for page in wizard.get("pages", []):
		if page.get("suggested", true):
			return false
	return not (wizard.get("pages", []) as Array).is_empty()


func _page_of(wizard: Dictionary, pointer: String) -> Dictionary:
	for page in wizard.get("pages", []):
		if String(page.get("pointer", "")) == pointer:
			return page
	return {}


func _describe(pages: Array) -> String:
	var parts: PackedStringArray = []
	for page in pages:
		parts.append("`%s` = %s%s" % [page.get("label", ""), page.get("value", ""), " (suggested)" if page.get("suggested", false) else ""])
	return "; ".join(parts)


## Every rule-list row's accessible name is its line, and there is one row per line.
func _check_rule_names(what: String) -> void:
	editor.refresh()
	var lines: PackedStringArray = _editor().get("prose", PackedStringArray())
	var rows: Array[Control] = editor.rule_controls()
	if lines.is_empty() or rows.size() != lines.size():
		_failures.append("%s: %d rule-list row(s) for %d line(s)" % [what, rows.size(), lines.size()])
		return
	for index in rows.size():
		if rows[index].accessibility_name != lines[index] or String(rows[index].get_meta("line")) != lines[index]:
			_failures.append("%s: rule-list row %d's accessible name is `%s`, its line `%s`" % [what, index, rows[index].accessibility_name, lines[index]])


## How many frames a key is held down, or a click is left to settle, in the camera check.
const KEY_FRAMES := 5


## The camera's keys belong to the text while a GUI control has keyboard focus, and come
## back to the camera once a click on the 3D view has released it (camera_rig.gd's header).
## Returns false when a step failed, with the reason recorded.
##
## A headless window is 64 pixels square, which the editor's panel covers whole, so the
## check gives the window the project's own size while it runs and puts it back after.
func _camera_keys() -> bool:
	var rig: Node3D = vista.rig
	if rig.target != null:
		_failures.append("the camera check needs free-look, but the rig follows %s" % rig.target)
		return false
	var window_size := get_window().size
	get_window().size = Vector2i(ProjectSettings.get_setting("display/window/size/viewport_width"), ProjectSettings.get_setting("display/window/size/viewport_height"))
	await get_tree().process_frame
	print("[watch-check] the camera's keys: the window %s, the view %s, the panel %s" % [get_window().size, get_viewport().get_visible_rect().size, editor.panel_rect()])
	var notes: TextEdit = editor._notes
	var kept := notes.text
	notes.grab_focus()
	await get_tree().process_frame
	var focused := get_viewport().gui_get_focus_owner()
	var before: Vector3 = rig.focus
	await _hold_key(KEY_W)
	var typed: Vector3 = rig.focus
	# The player's way back: a right-click on the view, left of the editor's panel, which
	# no control takes, so it reaches the rig's _unhandled_input.
	var click := InputEventMouseButton.new()
	click.button_index = MOUSE_BUTTON_RIGHT
	click.pressed = true
	click.position = get_viewport().get_visible_rect().size * Vector2(0.25, 0.5)
	click.global_position = click.position
	Input.parse_input_event(click)
	var release: InputEventMouseButton = click.duplicate()
	release.pressed = false
	Input.parse_input_event(release)
	for _frame in KEY_FRAMES:
		await get_tree().process_frame
	var holder := get_viewport().gui_get_focus_owner()
	await _hold_key(KEY_W)
	var moved: Vector3 = rig.focus
	notes.text = kept
	get_window().size = window_size
	await get_tree().process_frame
	if focused != notes:
		_failures.append("the notes box did not take keyboard focus: %s" % focused)
	if typed != before:
		_failures.append("W typed into the notes box moved the camera from %s to %s" % [before, typed])
	if holder != null:
		_failures.append("a right-click on the 3D view left the keyboard focus on %s" % holder)
	if moved == typed:
		_failures.append("W did not move the camera once the notes box had lost focus")
	print("[watch-check] the camera's keys: W in the notes box left the camera at %s; after a right-click on the view W moved it to %s" % [typed, moved])
	return _failures.is_empty()


## Holds `key` down for KEY_FRAMES frames, as a keyboard would, and lets it go.
func _hold_key(key: Key) -> void:
	var down := InputEventKey.new()
	down.keycode = key
	down.physical_keycode = key
	down.pressed = true
	Input.parse_input_event(down)
	for _frame in KEY_FRAMES:
		await get_tree().process_frame
	var up: InputEventKey = down.duplicate()
	up.pressed = false
	Input.parse_input_event(up)
	await get_tree().process_frame


## Save as... offers `name` (the text's own file, or "" for none), and is closed again
## (finding G). Returns false when it did not, with the reason recorded.
func _save_as_offers(name: String, what: String) -> bool:
	editor.open_save_as()
	await get_tree().process_frame
	var dialog: FileDialog = editor.save_dialog()
	var offered := dialog.current_file
	dialog.hide()
	await get_tree().process_frame
	if offered != name:
		_failures.append("%s: Save as... offered `%s`, not `%s`" % [what, offered, name])
		return false
	print("[watch-check] %s: Save as... offers `%s`" % [what, offered])
	return true


## The positive control for `_refused_in_the_push`, in round 1's Lull: the whole map framed
## as the lobby frames it, a left-click on the ground beside the commander through the map
## surface's own handler (`editor.map_click`, which `_unhandled_input` calls) opens a map
## menu and its ghost is asked for, so a ghost refused in the Push is the door's doing and
## not a click that missed. The
## click goes to the handler rather than through Input.parse_input_event because a headless
## window is 64 pixels square and all editor panel, which would take an injected click. The
## menu is closed again and the ghost the click asked for is waited out. Returns false when a
## step failed, with the reason recorded.
func _map_click_opens_a_menu() -> bool:
	vista.frame_whole_map()
	for _frame in KEY_FRAMES:
		await get_tree().process_frame
	var at := _ground_on_screen()
	if at.x < 0.0:
		_failures.append("the positive control: the camera does not show the ground beside the commander (%s)" % vista.my_commander_at())
		return false
	var taken: bool = editor.map_click(at, false)
	var opened: bool = editor.menu_open()
	# Finding I: Escape, through the engine's input, closes it.
	await _hold_key(KEY_ESCAPE)
	var escaped: bool = not editor.menu_open()
	editor.close_menu()
	if not taken or not opened:
		_failures.append("the positive control: a click on the map at %s opened no menu in the Lull (taken %s)" % [at, taken])
		return false
	if not escaped:
		_failures.append("Escape did not close the map menu")
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("ghost", {}).get("state", "")) != "waiting" and not s.get("pending", true), "the positive control's ghost never came back"):
		return false
	print("[watch-check] the Lull: a click on the map at %s opened its menu" % at)
	return true


## How many notches `_real_wheel` turns, and how far the ground under its pixel may drift.
const WHEEL_NOTCHES := 6
const WHEEL_DRIFT_PIXELS := 2.0


## **The wheel on the real view** (item 155 (3) 4): `client_check.gd` measures the wheel with a
## stand-in for the ground, because it has no view; this runs the view's own pick behind it. At
## the project's window size, with the whole map framed, on a pixel off the panel where the pick
## answers ground, notches sent through the engine's input reach the rig, bring it closer, and
## leave that ground under the pixel. The framing and the window are put back. Returns false when
## a step failed, with the reason recorded.
func _real_wheel() -> bool:
	var rig: Node3D = vista.rig
	if rig.target != null:
		_failures.append("the wheel check needs free-look, but the rig follows %s" % rig.target)
		return false
	var window_size := get_window().size
	get_window().size = Vector2i(ProjectSettings.get_setting("display/window/size/viewport_width"), ProjectSettings.get_setting("display/window/size/viewport_height"))
	await get_tree().process_frame
	vista.frame_whole_map()
	for _frame in KEY_FRAMES:
		await get_tree().process_frame
	# The middle of the view left of the panel: no control takes the wheel there, and with the
	# whole map framed the map is what is there.
	var at := Vector2(editor.panel_rect().position.x * 0.5, get_viewport().get_visible_rect().size.y * 0.5)
	var camera: Camera3D = rig.camera
	var picked: Dictionary = vista.ground_under(camera.project_ray_origin(at), camera.project_ray_normal(at))
	var ok := true
	if not picked.get("hit", false):
		_failures.append("the wheel check found no ground to wheel on: at %s, panel %s, pick %s" % [at, editor.panel_rect(), picked])
		ok = false
	else:
		var normal := camera.project_ray_normal(at)
		var origin := camera.project_ray_origin(at)
		# The ground the pick answered, on the ray through the pixel, which is what stays put.
		var ground: Vector3 = origin + normal * normal.dot(picked["at"] - origin)
		var before: float = rig.distance
		_mouse_move(at)
		for _notch in WHEEL_NOTCHES:
			_mouse_button(at, MOUSE_BUTTON_WHEEL_UP, true)
			_mouse_button(at, MOUSE_BUTTON_WHEEL_UP, false)
			await get_tree().process_frame
		for _frame in KEY_FRAMES:
			await get_tree().process_frame
		var drift: float = camera.unproject_position(ground).distance_to(at)
		print("[watch-check] the wheel on the real view: %d notches at %s took the camera from %.0f to %.0f, the ground %.2f px from the pixel" % [WHEEL_NOTCHES, at, before, rig.distance, drift])
		if rig.distance >= before:
			_failures.append("%d wheel notches at %s did not bring the camera in: %.1f from %.1f" % [WHEEL_NOTCHES, at, rig.distance, before])
			ok = false
		if drift >= WHEEL_DRIFT_PIXELS:
			_failures.append("the wheel moved the ground under %s by %.2f px" % [at, drift])
			ok = false
	vista.frame_whole_map()
	get_window().size = window_size
	await get_tree().process_frame
	return ok


## Where the camera shows the ground 8 voxels west of this seat's commander, the spot
## `_one_click`'s menu case clicks, or (-1, -1) when it is behind the camera or off screen.
func _ground_on_screen() -> Vector2:
	var commander: Vector3i = vista.my_commander_at()
	var camera: Camera3D = vista.rig.camera
	var where: Vector3 = editor._world(Vector3i(commander.x - 8, commander.y, commander.z))
	if commander.x < 0 or camera.is_position_behind(where):
		return Vector2(-1, -1)
	var at := camera.unproject_position(where)
	if not get_viewport().get_visible_rect().has_point(at):
		return Vector2(-1, -1)
	return at


## In the Push (finding K): the same left-click through the map surface's own handler opens
## the map's menu, as it always did (item 139 (2)); the ghost it asks for and the menu's Go
## are refused at once with the Push's reason; nothing the editor owes is left waiting; and
## a submission is refused at once too. Returns false when a step failed, with the reason
## recorded.
func _refused_in_the_push() -> bool:
	var at := _ground_on_screen()
	if at.x < 0.0:
		_failures.append("the Push: the camera does not show the ground beside the commander")
		return false
	var taken: bool = editor.map_click(at, false)
	var opened: bool = editor.menu_open()
	await get_tree().process_frame
	# Still open after a redraw: a menu whose ghost was refused from the start stays, saying why
	# (fix pass 2 item 1: the L4 fix closed it one frame after it opened).
	var stayed: bool = editor.menu_open()
	var after := _editor()
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	var went := _editor()
	var submitted: bool = vista.bridge.editor_submit()
	var refused := _editor()
	if _state().get("phase", "") != "push":
		_failures.append("the Push ended before its map was checked: %s" % _state())
		return false
	if String(after.get("door", "")) != "push":
		_failures.append("the editor's door in the Push is `%s`, not push" % after.get("door"))
	if not taken or not opened:
		_failures.append("a click on the map at %s opened no menu in the Push (taken %s)" % [at, taken])
	elif not stayed:
		_failures.append("the Push's map menu closed one frame after it opened")
	if not (after.get("ghost", {}) as Dictionary).is_empty() or String(after.get("status_key", "")) != "closed_push":
		_failures.append("the click's ghost was not refused at once in the Push: %s" % after)
	if String(went.get("status_key", "")) != "closed_push" or went.get("pending", true) or went.get("busy", true):
		_failures.append("the menu's Go was not refused at once in the Push: %s" % went)
	if submitted or String(refused.get("status_key", "")) != "closed_push" or refused.get("pending", true):
		_failures.append("a submission in the Push was not refused at once: %s" % refused)
	print("[watch-check] the Push: a click on the map at %s opened its menu (door %s); its ghost, its Go and a submission were refused at once: %s" % [at, after.get("door"), Strings.text("status_" + String(refused.get("status_key", "")))])
	return _failures.is_empty()


## The buttons F1 names, and the three cases each is clicked in.
const CLICKED := ["fix", "save", "submit"]
const CASES := ["redraw", "menu", "scroll"]
## How many wheel notches one scroll of the panel is.
const NOTCHES := 3
## Where the map menu is opened for the menu case: on the 3D view, clear of the panel.
const MENU_AT := Vector2(300, 300)


## **The first click lands** (F1, decisions-log item 126 (3); S1's plan, task `fixc`): one
## injected click on Submit, a Fix button and Save reaches its handler exactly once, with
## the rows redrawn between its press and its release, with a map menu open, and after a
## scroll of the panel. Submit goes last, on the committed playbook, so the Lull's sealed
## order is one QUICK and FULL accept. Returns false when a step failed, with the reason
## recorded.
func _first_click_lands() -> bool:
	var window_size := get_window().size
	get_window().size = Vector2i(ProjectSettings.get_setting("display/window/size/viewport_width"), ProjectSettings.get_setting("display/window/size/viewport_height"))
	await get_tree().process_frame
	# Save with no file opens Save as... (F6: "Save with no file does nothing visible"):
	# `_one_click` opens each file from its bytes, so the text has no file to Save to.
	var ok := true
	for name in CLICKED:
		for case in CASES:
			if not await _one_click(name, case):
				ok = false
				break
		if not ok:
			break
	editor.close_menu()
	get_window().size = window_size
	await get_tree().process_frame
	return ok and _failures.is_empty()


## Opens what `name`'s button needs (a Fix needs the file with a Fix), sets up `case`, clicks
## the button once, and checks its handler ran exactly once.
func _one_click(name: String, case: String) -> bool:
	var what := "%s, %s" % [name, case]
	var file := NEEDS_A_FIX if name == "fix" else COMMITTED
	# Opened from its bytes, as a carried draft is, so the editor has no file to Save to.
	editor.load_bytes(FileAccess.get_file_as_bytes(file))
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("has_text", false) and s.get("rows_current", false) and not s.get("busy", true) and not s.get("pending", false), "%s: %s never opened" % [what, file]):
		return false
	await get_tree().process_frame
	if not get_viewport().get_visible_rect().encloses(editor.size_rect()) or not editor._size_box.visible:
		_failures.append("%s: the size meter is not on screen: %s" % [what, editor.size_rect()])
		return false
	if name == "fix" and editor.panel_button("fix") == null:
		_failures.append("%s: no Fix button was drawn for %s" % [what, file])
		return false
	var revision := _count(_editor(), "revision")
	if case == "menu":
		var commander: Vector3i = vista.my_commander_at()
		editor._on_ground_clicked(Vector3i(commander.x - 8, commander.y, commander.z), MENU_AT)
		if not await _editor_until(func(s: Dictionary) -> bool: return s.get("ghost", {}).get("state", "waiting") != "waiting" and not s.get("busy", true), "%s: the ghost under the menu never came back" % what):
			return false
		if not editor.menu_open():
			_failures.append("%s: the map menu did not open" % what)
			return false
	elif case == "scroll":
		if not await _scroll_panel(what, name):
			return false
	var button: Button = editor.panel_button(name)
	if button == null or button.disabled:
		_failures.append("%s: the button is not there to click (%s)" % [what, button])
		return false
	var before: Dictionary = editor.click_counts()[name]
	var at := button.get_global_rect().get_center()
	var scrolled: int = editor.panel_scroll().scroll_vertical
	_mouse_move(at)
	_mouse_button(at, MOUSE_BUTTON_LEFT, true)
	await get_tree().process_frame
	if case == "redraw":
		# Something the gateway answers changes the editor's state, and the panel redraws
		# while the button is held: the rows as they were must keep their buttons.
		var saved := int(_editor().get("notes_saved", -1))
		vista.bridge.editor_save_notes(NOTES)
		if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("notes_saved", -1)) == NOTES.length() and not s.get("pending", false), "%s: the redraw's answer never came (%d before)" % [what, saved]):
			return false
		for _frame in KEY_FRAMES:
			await get_tree().process_frame
	_mouse_button(at, MOUSE_BUTTON_LEFT, false)
	for _frame in KEY_FRAMES:
		await get_tree().process_frame
	var after: Dictionary = editor.click_counts()[name]
	var downs := int(after["down"]) - int(before["down"])
	var handled := int(after["pressed"]) - int(before["pressed"])
	print("[watch-check] first click, %s: button_down %d, pressed %d (clicked at %s with the panel scrolled %d px)" % [what, downs, handled, at, scrolled])
	if downs != 1 or handled != 1:
		_failures.append("%s: one click reached button_down %d time(s) and its handler %d time(s), not once each" % [what, downs, handled])
		return false
	if case == "menu" and editor.menu_open():
		_failures.append("%s: the map menu stayed open after a click on the panel" % what)
		return false
	match name:
		"fix":
			if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > revision and String(s.get("status_key", "")) == "fixed" and s.get("rows_current", false) and not s.get("busy", true), "%s: the Fix was never applied" % what):
				return false
		"save":
			var dialog: FileDialog = editor.save_dialog()
			if not dialog.visible:
				_failures.append("%s: Save with no file did not open Save as..." % what)
				return false
			dialog.hide()
			await get_tree().process_frame
		"submit":
			if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("status_key", "")) in ["submitted", "submit_refused"] and not s.get("pending", true), "%s: the submission never came back" % what):
				return false
			if not _editor().get("accepted", false):
				_failures.append("%s: the submission was not accepted: %s" % [what, _editor().get("rows", [])])
				return false
	editor.panel_scroll().scroll_vertical = 0
	await get_tree().process_frame
	return true


## Scrolls the panel with the wheel, as a mouse would: NOTCHES notches down, and back up
## again only if that scrolled `name`'s button out of sight (Submit and Save sit at the
## panel's top), so a Fix button is clicked where the scroll left it. The panel must have
## moved. The layout settles for KEY_FRAMES frames before the button is found, as it has by
## the time a person sees where the button went.
func _scroll_panel(what: String, name: String) -> bool:
	var inside: Vector2 = editor.panel_rect().get_center()
	_mouse_move(inside)
	await _wheel(inside, MOUSE_BUTTON_WHEEL_DOWN)
	if editor.panel_scroll().scroll_vertical <= 0:
		_failures.append("%s: the wheel did not scroll the panel" % what)
		return false
	var button: Button = editor.panel_button(name)
	var seen := get_viewport().get_visible_rect()
	if button == null or not seen.has_point(button.get_global_rect().get_center()):
		await _wheel(inside, MOUSE_BUTTON_WHEEL_UP)
	return true


## NOTCHES notches of the wheel `button` at `at`, then KEY_FRAMES frames for the layout.
func _wheel(at: Vector2, button: MouseButton) -> void:
	for _notch in NOTCHES:
		_mouse_button(at, button, true)
		_mouse_button(at, button, false)
		await get_tree().process_frame
	for _frame in KEY_FRAMES:
		await get_tree().process_frame


func _mouse_move(at: Vector2) -> void:
	var motion := InputEventMouseMotion.new()
	motion.position = at
	motion.global_position = at
	Input.parse_input_event(motion)


func _mouse_button(at: Vector2, button: MouseButton, pressed: bool) -> void:
	var event := InputEventMouseButton.new()
	event.button_index = button
	event.pressed = pressed
	event.position = at
	event.global_position = at
	Input.parse_input_event(event)


## **New, then one Go here, qualifies at QUICK** (S3's `ed3b`, fix pass 2 item 4): New puts the
## shipped playbook on screen, whose only error is its empty route; one Go here through the map
## surface's own handlers (eight voxels west of the commander, as the committed playbook walks)
## makes the live gateway's QUICK qualify it. Returns false when a step failed, with the reason
## recorded.
func _new_then_go_here() -> bool:
	if not editor.new_playbook():
		_failures.append("New was refused: %s" % _editor().get("status_key", ""))
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("status_key", "")) == "new_opened" and s.get("rows_current", false) and not s.get("busy", true), "New's playbook was never checked"):
		return false
	var codes: Array = []
	for row in _editor().get("rows", []):
		if String(row.get("severity", "")) == "error":
			codes.append(String(row.get("code", "")))
	if codes != ["E0102"]:
		_failures.append("New's playbook has errors %s, where only the empty route's E0102 was due" % [codes])
		return false
	var commander: Vector3i = vista.my_commander_at()
	var before := _count(_editor(), "revision")
	editor._on_ground_clicked(Vector3i(commander.x - 8, commander.y, commander.z), Vector2.ZERO)
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and s.get("rows_current", false) and not s.get("busy", true), "New's Go here was never patched and checked"):
		return false
	if not _editor().get("qualifies", false):
		_failures.append("New, then one Go here, does not qualify at QUICK: %s" % [_editor().get("rows", [])])
		return false
	print("[watch-check] New, then one Go here: QUICK qualifies it")
	return true


## The editor's half of the run (pull request 1). Returns false when a step failed, with the
## reason recorded.
func _edit() -> bool:
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("notes_known", false) and s.get("drafts_known", false), "the notebook and the drafts were never read"):
		return false

	# 1. Load refuses an out-of-vocabulary construct, with its code and pointer.
	editor.load_file(OUT_OF_VOCABULARY)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("verdict", "") == "refused", "Load never refused the out-of-vocabulary file"):
		return false
	var refused := _editor()
	editor.refresh()
	var shown: String = editor.status_text()
	if refused.get("has_text", true):
		_failures.append("Load opened a file it refused")
	if refused.get("status_detail", "") != "E0003 /declarative/route/0/set_flag" or not ("E0003" in shown and "/declarative/route/0/set_flag" in shown):
		_failures.append("the refusal did not show the code and the pointer: %s" % shown)
	_check_accessible_names("the Load refusal")
	print("[watch-check] Load refused the out-of-vocabulary file: %s" % shown)

	# 1b. New, then one Go here, verifies clean at QUICK against the live gateway (the owner's
	# ruling of 2026-11-17: the new playbook carries the verifier's own on_death and fallback, so
	# only a route is missing). Load, next, replaces the text.
	if not await _new_then_go_here():
		return false

	# 2. A Fix button applies the verifier's own patch through patch_plan.
	editor.load_file(NEEDS_A_FIX)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("has_text", false) and s.get("rows_current", false) and not s.get("busy", true), "the file with a Fix never opened"):
		return false
	var rows: Array = _editor().get("rows", [])
	var unfixed := _count(_editor(), "revision")
	var fixed := false
	for index in rows.size():
		if rows[index].get("code", "") == "E0108" and not (rows[index].get("fixes", []) as Array).is_empty():
			fixed = editor.fix(index, 0)
	if not fixed:
		_failures.append("no Fix button was offered for E0108: %s" % [rows])
		return false
	_check_accessible_names("the Fix rows")
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > unfixed and s.get("rows_current", false) and s.get("qualifies", false), "the Fix did not leave a playbook QUICK accepts"):
		return false
	print("[watch-check] Fix applied the verifier's patch; QUICK now qualifies it")

	# 3. The committed playbook opens and saves back byte for byte.
	var committed := COMMITTED
	var original := FileAccess.get_file_as_bytes(committed)
	if original.is_empty():
		_failures.append("could not read %s" % committed)
		return false
	editor.load_file(committed)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("has_text", false) and s.get("verdict", "") != "refused" and s.get("rows_current", false) and not s.get("busy", true) and vista.bridge.editor_bytes() == original, "the committed playbook never opened"):
		return false
	if not _editor().get("qualifies", false):
		_failures.append("QUICK did not qualify the committed playbook: %s" % [_editor().get("rows", [])])
	if not await _save_as_offers(COMMITTED.get_file(), "the opened committed playbook"):
		return false
	if not editor.save_file(SAVED) or FileAccess.get_file_as_bytes(SAVED) != original:
		_failures.append("opening and saving the committed playbook did not give its bytes back")
	if editor.text_file() != SAVED:
		_failures.append("the saved file is not the text's file: `%s`" % editor.text_file())
	if not await _save_as_offers(SAVED.get_file(), "the saved playbook"):
		return false
	print("[watch-check] opened and saved %s byte for byte" % COMMITTED)
	var loaded_revision := _count(_editor(), "revision")

	# 4. The placement ghost, per click; the placement taken, and undone.
	var commander: Vector3i = vista.my_commander_at()
	if commander.x < 0:
		_failures.append("the view shows no commander of this seat's")
		return false
	var spot := Vector3i(commander.x - 8, commander.y, commander.z)
	# Through the map surface's own handlers: a ground click opens the menu and asks for the
	# ghost, and the menu's Place beacon takes it.
	editor._on_ground_clicked(spot, Vector2.ZERO)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("ghost", {}).get("state", "waiting") != "waiting", "the placement ghost never came back"):
		return false
	print("[watch-check] ghost at %s: %s %s" % [spot, _editor()["ghost"].get("state"), _editor()["ghost"].get("sentence")])
	if String(_editor()["ghost"].get("state")) != "legal":
		_failures.append("the ghost at %s is not legal: %s" % [spot, _editor()["ghost"]])
		return false
	editor._on_menu(editor.ITEM_PLACE)
	editor.close_menu()
	if _count(_editor(), "revision") != loaded_revision + 1:
		_failures.append("the checked placement was not taken at once")
		return false
	vista.bridge.editor_undo()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") == loaded_revision + 2 and not s.get("busy", true), "Undo never came back"):
		return false
	if vista.bridge.editor_bytes() != original:
		_failures.append("Undo did not give the loaded bytes back")
	print("[watch-check] placement taken and undone; the bytes are the loaded ones again")

	# 5. One map action: Alt-click on the seat's own beacon, Go here, the nearest own beacon.
	var own := {}
	for beacon in vista.beacons():
		if beacon["owner"] == vista.my_seat:
			own = beacon
	if own.is_empty():
		_failures.append("the view shows no beacon of this seat's")
		return false
	var before := _count(_editor(), "revision")
	# Through the map surface's own handlers: an Alt-click on the beacon (which also asks
	# get_beacon about it), whose menu's first target is the nearest own beacon, then Go here.
	editor._on_beacon_clicked(own, true, Vector2.ZERO)
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("beacon_prose", "")) != "", "get_beacon never described %s" % own["id"]):
		return false
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and s.get("verdict", "") == "full" and s.get("route", {}).get("current", false), "the map action was never patched, checked and priced"):
		return false
	var after := _editor()
	if not after.get("qualifies", false):
		_failures.append("FULL did not qualify the edited playbook: %s" % [after.get("rows", [])])
	var route: Dictionary = after.get("route", {})
	print("[watch-check] map action applied; FULL qualifies it; route %s point(s), legs %s" % [(route.get("points", []) as Array).size(), route.get("legs")])
	if (route.get("points", []) as Array).size() < 2 or not route.get("reachable", false):
		_failures.append("the edited route was not priced: %s" % [route])
	if not ("\"nearest\"" in vista.bridge.editor_bytes().get_string_from_utf8()):
		_failures.append("the Alt-click did not make the step's target the nearest own beacon")

	# 6. The notebook and a draft.
	vista.bridge.editor_save_notes(NOTES)
	vista.bridge.editor_save_draft("watch check")
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("notes_saved", -1)) == NOTES.length() and _has_draft(s, "editor"), "the notebook or the draft was never saved"):
		return false

	# 7. Submit; the bytes that went are the expected ones, and gamectl verify accepts them.
	editor.submit()
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("submitted", false) and not s.get("pending", true), "the submission never came back"):
		return false
	if not _editor().get("accepted", false):
		_failures.append("the submission was not accepted: %s" % [_editor().get("rows", [])])
		return false
	var sealed: PackedByteArray = vista.bridge.editor_sealed_bytes()
	var expected := FileAccess.get_file_as_bytes(EXPECTED)
	if sealed != expected:
		_failures.append("the submitted bytes are not %s (%d bytes against %d)" % [EXPECTED, sealed.size(), expected.size()])
	if not editor.save_file(SAVED):
		_failures.append("the submitted playbook could not be saved")
		return false
	var output := []
	var verified := OS.execute(_gamectl, PackedStringArray(["--root", _root, "verify", ProjectSettings.globalize_path(SAVED)]), output, true)
	if verified != 0:
		_failures.append("gamectl verify refused the saved file (exit %d): %s" % [verified, "".join(output)])
	print("[watch-check] submitted and accepted; the bytes are %s's; gamectl verify exit %d" % [EXPECTED, verified])
	if int(_editor().get("refusals", 0)) != 0:
		_failures.append("the gateway refused %s of the editor's calls" % _editor().get("refusals"))
	return true


## Every row on screen has its plain sentence as its accessible name.
func _check_accessible_names(what: String) -> void:
	editor.refresh()
	var controls: Array[Control] = editor.row_controls()
	if controls.is_empty():
		_failures.append("%s: no rows were drawn" % what)
	for control in controls:
		var sentence := String(control.get_meta("sentence"))
		if sentence == "" or control.accessibility_name != sentence:
			_failures.append("%s: a row's accessible name is `%s`, its sentence `%s`" % [what, control.accessibility_name, sentence])


func _has_draft(state: Dictionary, id: String) -> bool:
	for draft in state.get("drafts", []):
		if draft.get("id", "") == id:
			return true
	return false


## What must hold of one client's bridge, whichever host it watched.
## The drops of `link`'s connection itself: its drops less its closes on a message, which the
## planning wire's big loads make on purpose and which spend no reconnect (fix pass 4).
func _connection_drops(state: Dictionary, link: String) -> int:
	return _count(state, "drops_" + link) - _count(state, "message_drops_" + link)


func _check_bridge(who: String) -> void:
	var state := _state()
	print("[watch-check] %s's final state: %s" % [who, state])
	print("[watch-check] %s: %d event rows; %s" % [who, _events, vista.bridge.panic_report()])
	if not _host_killed and (_connection_drops(state, "admin") != 0 or _connection_drops(state, "seat") != 0):
		_failures.append("%s: a connection dropped: admin %s, seat %s (closes on a message: %s, %s)" % [who, state.get("drops_admin"), state.get("drops_seat"), state.get("message_drops_admin"), state.get("message_drops_seat")])
	if vista.bridge.caught_panics() != 0:
		_failures.append("%s: %d panic(s) were caught at the bridge boundary" % [who, vista.bridge.caught_panics()])
	if int(state.get("view_refusals", 0)) != 0:
		_failures.append("%s: the bridge refused %s view page(s)" % [who, state.get("view_refusals")])
	if not _refusals.is_empty():
		_failures.append("%s: the gateway refused %d call(s): %s" % [who, _refusals.size(), "; ".join(_refusals)])
	if vista.marker_count() == 0:
		_failures.append("%s: no entity was drawn" % who)


func _finish() -> void:
	_check_bridge("the Probation client" if _probation else ("the resumed client" if _resumed else "the first client"))
	_ended = true
	var pid: int = link.host_pid()
	link.quit()
	if pid >= 0:
		if not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
			_failures.append("the host (pid %d) was still running after its standard input closed" % pid)
		else:
			print("[watch-check] the host exited once its standard input closed")
	if _failures.is_empty() and _probation:
		await _silent_host()
	if _failures.is_empty() and _probation:
		var carried: Dictionary = Engine.get_meta(CARRY)
		print("[watch-check] OK (host pids %d, %d and %d)" % [int(carried.get("first_pid", -1)), int(carried.get("resumed_pid", -1)), pid])
		get_tree().quit(0)
	elif _failures.is_empty() and _resumed:
		# On to the Probation leg, with nothing carried but the expectations and the pids.
		var carried: Dictionary = Engine.get_meta(CARRY)
		carried["stage"] = "probation"
		carried["resumed_pid"] = pid
		carried["failures"] = _failures
		carried["refusals"] = _refusals
		carried["events"] = _events
		Engine.set_meta(CARRY, carried)
		print("[watch-check] the resumed host exited; changing scene for the Probation leg")
		get_tree().change_scene_to_file("res://scenes/watch_check.tscn")
	elif _failures.is_empty():
		# A run that stops before the resume and Probation legs is not a pass.
		_failures.append("the check ended before its resume leg")
		_report_failures()
	else:
		_report_failures()


func _report_failures() -> void:
	for failure in _failures:
		printerr("[watch-check] FAILED: %s" % failure)
	get_tree().quit(1)


func _on_failed(reason: String) -> void:
	if _expecting_refusal:
		_refused_reason = reason
		return
	_failures.append("the host link failed: %s" % reason)


func _on_received(answer: Dictionary) -> void:
	var refusal := String(answer.get("error", ""))
	if refusal != "":
		_refusals.append(refusal)
	_events += (answer.get("events", []) as Array).size()
	if answer.has("view"):
		if (answer["view"] as Dictionary).is_empty():
			_failures.append("a get_view page did not decode (the reason is in the log)")
		else:
			vista.take_view(answer["view"])


## The planning wire against the real gateway (S3's `fixq-c`): see the header.
func _planning_wire() -> bool:
	var original := FileAccess.get_file_as_bytes(COMMITTED)
	editor.load_file(COMMITTED)
	if not await _editor_until(func(s: Dictionary) -> bool: return vista.bridge.editor_bytes() == original and s.get("prose_current", false) and s.get("rows_current", false) and s.get("rules_editable", false) and s.get("vocabulary", {}).get("known", false), "the planning wire: the committed playbook never rendered"):
		return false
	var depth := _count(_editor(), "undo_depth")
	# A duration, typed in seconds.
	var timeout := _chip_at(_editor(), "/timeout_ms")
	var chip: Dictionary = _editor().get("rule_list", {}).get("chips", [])[timeout] if timeout >= 0 else {}
	if String(chip.get("editor", "")) != "number" or String(chip.get("display_unit", "")) != "s" or String(chip.get("value_text", "")) == "":
		_failures.append("the planning wire: the duration chip is not a number typed in seconds: %s" % [chip])
		return false
	editor._on_rule_chip(timeout)
	await get_tree().process_frame
	if editor._number_field.field.text != String(chip.get("value_text", "")) or editor._number_field.unit.text != "s":
		_failures.append("the planning wire: the duration's field shows `%s %s`, not the gateway's `%s s`" % [editor._number_field.field.text, editor._number_field.unit.text, chip.get("value_text", "")])
	var before := _count(_editor(), "revision")
	editor._number_field.field.text = "3"
	editor._number_field.send.pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and not s.get("busy", true) and s.get("prose_current", false), "the planning wire: the duration's \"3\" never landed"):
		return false
	var typed: String = vista.bridge.editor_bytes().get_string_from_utf8()
	if not ("\"timeout_ms\": 3000" in typed):
		_failures.append("the planning wire: \"3\" seconds did not land as 3000 ms")
	editor._number_popup.hide()
	await _editor_until(func(s: Dictionary) -> bool: return s.get("rules_editable", false), "the planning wire: the list never took an edit again")
	# Pastes against the real gateway (fix pass 2, item 1e): every one leaves the seat link up
	# with its drops unchanged, and the leg then seals, and Ready ends the Lull as ever.
	if not await _pastes():
		return false
	if not await _closes_on_a_message():
		return false
	# Add a rule, through its button; its empty slot takes a group; Remove takes it out.
	var rules_before: int = _editor().get("rule_list", {}).get("rules", []).size()
	var add := _rule_button("rule_add")
	if add == null:
		_failures.append("the planning wire: no Add a rule button")
		return false
	before = _count(_editor(), "revision")
	add.pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and s.get("rules_editable", false) and s.get("rule_list", {}).get("rules", []).size() == rules_before + 1, "the planning wire: Add a rule never landed a rule"):
		return false
	var slots: Array = _editor().get("rule_list", {}).get("slots", [])
	var slot := -1
	for index in slots.size():
		if String(slots[index].get("state", "")) == "empty" and int(slots[index].get("rule", -1)) == rules_before and slots[index].get("pickable", false):
			slot = index
	if slot < 0:
		_failures.append("the planning wire: the new rule has no empty slot for the picker: %s" % [slots])
		return false
	before = _count(_editor(), "revision")
	if not vista.bridge.editor_choose_group(slot, String(slots[slot].get("pointer", "")), int(_editor().get("rules_rendering", -1)), 0):
		_failures.append("the planning wire: the group pick was refused: %s" % _editor().get("status_key", ""))
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and s.get("rules_editable", false), "the planning wire: the group never landed"):
		return false
	if not ("\"all\"" in vista.bridge.editor_bytes().get_string_from_utf8()):
		_failures.append("the planning wire: the group pick wrote no `all`")
	var remove: Button = null
	for button in editor._rules_box.find_children("*", "Button", true, false):
		if (button as Button).has_meta("rule_remove") and int(button.get_meta("rule_remove")) == rules_before:
			remove = button
	if remove == null:
		_failures.append("the planning wire: the new rule has no Remove button")
		return false
	before = _count(_editor(), "revision")
	remove.pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "revision") > before and s.get("rules_editable", false) and s.get("rule_list", {}).get("rules", []).size() == rules_before, "the planning wire: Remove never took the rule out"):
		return false
	# A wall on the seat's core, by two ground clicks beside the commander.
	var core := ""
	for beacon in vista.beacons():
		if String(beacon["owner"]) == vista.my_seat:
			core = String(beacon["id"])
	vista.bridge.editor_describe_beacon(core)
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("beacon_detail", {}).get("id", "")) == core, "the planning wire: the core's detail never came"):
		return false
	# The core's settings in the gateway's words: a choice by its title, a number in its unit.
	editor._visit_beacon = core
	editor._visit_drawn = ""
	editor._fill_visit_menu(_editor())
	var drawn: Array[String] = []
	for submenu in editor._visit_submenus:
		for item in (submenu as PopupMenu).item_count:
			drawn.append((submenu as PopupMenu).get_item_text(item))
	for setting in _editor().get("beacon_detail", {}).get("settings", []):
		var said := Strings.text("menu_setting", {"title": setting.get("title", ""), "value": editor.setting_value_text(setting)})
		if not drawn.has(said) or "\"" in said:
			_failures.append("the planning wire: the setting `%s` is not drawn in the gateway's words: %s" % [said, drawn])
	var blueprints: Array = _editor().get("vocabulary", {}).get("blueprints", [])
	var wall := -1
	for index in blueprints.size():
		if String(blueprints[index].get("anchor_shape", "")) == "box":
			wall = index
	var at: Vector3i = vista.my_commander_at()
	before = _count(_editor(), "revision")
	if wall < 0 or not vista.bridge.editor_visit_row(core, "add_target", wall):
		_failures.append("the planning wire: Add a build target took no wall: %s" % _editor().get("status_key", ""))
		return false
	vista.bridge.editor_pick({"voxel": at + Vector3i(2, 2, 0)})
	vista.bridge.editor_pick({"voxel": at + Vector3i(4, 3, 0)})
	if not await _editor_until(func(s: Dictionary) -> bool: return (_count(s, "revision") > before or String(s.get("status_key", "")) == "gateway_refused") and not s.get("busy", true) and not s.get("pending", true), "the planning wire: the wall's box never came"):
		return false
	if not ("\"wall_box\"" in vista.bridge.editor_bytes().get_string_from_utf8()):
		_failures.append("the planning wire: the wall's row holds no box: %s %s" % [_editor().get("status_key", ""), _editor().get("status_detail", "")])
	# The Queue's own list.
	var queued: Array = _editor().get("vocabulary", {}).get("queue_blueprints", [])
	editor._visit_beacon = core
	editor.open_entry_list(editor.ITEM_LIST_QUEUE, _editor().get("vocabulary", {}).get(String(editor.ENTRY_LISTS[editor.ITEM_LIST_QUEUE][1]), []), Rect2(Vector2(200, 300), Vector2(180, 200)))
	await get_tree().process_frame
	if editor.entry_rows().size() != queued.size() or queued.is_empty():
		_failures.append("the planning wire: Queue a structure lists %d rows of the Queue's %d" % [editor.entry_rows().size(), queued.size()])
	editor._entry_popup.hide()
	# Undo takes every one of them back to the committed bytes.
	while _count(_editor(), "undo_depth") > depth:
		var undone := _count(_editor(), "undo_depth")
		vista.bridge.editor_undo()
		if not await _editor_until(func(s: Dictionary) -> bool: return _count(s, "undo_depth") < undone and not s.get("busy", true) and not s.get("pending", true), "the planning wire: Undo never came back"):
			return false
	if vista.bridge.editor_bytes() != original:
		_failures.append("the planning wire: Undo did not give the committed bytes back")
	# Sealed again after the paste: the submission is accepted, as any other.
	editor.submit()
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("accepted", false) and not s.get("pending", true) and vista.bridge.editor_sealed_bytes() == original, "the planning wire: the seal after the paste was never accepted"):
		return false
	# The same text sent again, as the editor re-sends a submission lost with its connection, is
	# the same seal (fix pass 7, item 7): `submit_plan` replaces the seat's seal with this text's.
	if not editor.submit() or not _editor().get("pending", false):
		_failures.append("the planning wire: the same text was not taken for a second submission")
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("accepted", false) and not s.get("pending", true) and vista.bridge.editor_sealed_bytes() == original, "the planning wire: the same text submitted again was not the same seal"):
		return false
	print("[watch-check] the planning wire: the same text submitted twice, accepted twice, the same seal")
	print("[watch-check] the planning wire: a duration typed in seconds landed, titles drawn, a rule added, filled with a group and removed, a wall's box composed, the Queue's own list, all undone")
	return true


## Pastes into the duration field, against the real gateway (S3's `fixq-c`, fix pass 2, item
## 1e): about 100 KB, under the cap and past Godot's default outbound buffer (sent; the gateway
## refuses the value in its words); exactly the cap (sent, on a peer sized for it); the cap and one
## byte more (refused before it goes); 90 000 three-byte characters (over the cap in bytes); and a
## text of quotes and backslashes over the cap only once JSON escapes it. The seat link stays up
## and its drops stay as they were through every one; nothing waits "Sending...".
func _pastes() -> bool:
	var cap := _count(vista.bridge.watch_transport_limits(), "message_bytes")
	var drops := _count(_state(), "drops_seat")
	var paste := func(text: String) -> Dictionary:
		var at := _chip_at(_editor(), "/timeout_ms")
		editor._on_rule_chip(at)
		await get_tree().process_frame
		editor._number_field.field.text = text
		editor._number_field.send.pressed.emit()
		if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("field", {}).get("state", "")) in ["refused", "landed"] and not s.get("busy", true), "the planning wire: a paste of %d characters never settled" % text.length()):
			return {}
		var field: Dictionary = _editor().get("field", {})
		editor._number_popup.hide()
		await _editor_until(func(s: Dictionary) -> bool: return s.get("rules_editable", false), "the planning wire: the list never took an edit after a paste")
		return field
	# The frame's own bytes around the typed text, measured from a refusal that names them.
	var probe: Dictionary = await paste.call("1".repeat(cap))
	if String(probe.get("key", "")) != "status_call_too_large":
		_failures.append("the planning wire: a paste of the cap's length went out: %s" % probe)
		return false
	var around := int(String(probe.get("detail", "0"))) - cap
	var cases := [
		["about 100 KB", "1".repeat(100000), "gateway"],
		["exactly the cap", "1".repeat(cap - around), "sent"],
		["the cap and one byte", "1".repeat(cap - around + 1), "unsent"],
		["90 000 three-byte characters", "€".repeat(90000), "unsent"],
		["quotes and backslashes", "\"\\".repeat(70000), "unsent"],
	]
	for case in cases:
		var unsent := _count(_state(), "unsent_seat")
		var field: Dictionary = await paste.call(String(case[1]))
		if field.is_empty():
			return false
		var key := String(field.get("key", ""))
		match String(case[2]):
			"gateway":
				if key != "rule_refused":
					_failures.append("the planning wire: %s was not the gateway's to refuse: %s" % [case[0], field])
				_take_asked_refusal(String(field.get("detail", "")))
			"sent":
				if _count(_state(), "unsent_seat") != unsent or key == "status_call_too_large" or key == "status_call_send_failed":
					_failures.append("the planning wire: %s did not go out: %s" % [case[0], field])
				# The gateway reads it, and may refuse the value in its words: a refusal asked for.
				if key == "rule_refused":
					_take_asked_refusal(String(field.get("detail", "")))
			"unsent":
				if key != "status_call_too_large" or _count(_state(), "unsent_seat") != unsent + 1:
					_failures.append("the planning wire: %s was not refused before it went: %s" % [case[0], field])
		if _count(_state(), "drops_seat") != drops or _count(_state(), "send_failures_seat") != 0:
			_failures.append("the planning wire: %s dropped the seat link or the transport refused it: %s" % [case[0], _state()])
		print("[watch-check] the planning wire: %s: %s (`%s`), drops %d" % [case[0], key, String(field.get("detail", "")).left(80), _count(_state(), "drops_seat")])
	return true


## A refusal the planning wire asked for, taken off `_refusals` (where `_on_received` put it) and
## counted, so the run's own "the gateway refused no call" stays about the calls nobody asked it to
## refuse. A refusal that never arrived is this check's failure.
func _take_asked_refusal(detail: String) -> void:
	for index in _refusals.size():
		if _refusals[index] == detail:
			_refusals.remove_at(index)
			_asked_refusals += 1
			return
	_failures.append("the planning wire: the gateway's refusal `%s` never reached the feed of answers" % detail.left(80))


## **Closes on a message do not end a playable match** (S3's `fixq-c`, fix passes 4 to 6, item
## 1; re-aimed by S3's `vcap`). Review 3's playbook of 18 000 bare holds (216 056 bytes, under the
## cap) is loaded six times with no wait between the loads, as a player might click. Since `vcap`
## a report lists at most four of a code (the verifier's `MAX_PER_CODE`), so the load's own QUICK
## answer fits (8 583 bytes as the gateway writes it) and the load lands; its `render_plan`
## answer does not (15 690 780 bytes, past the client's 4 MiB inbound buffer: lane `outcap`'s
## finding), so the render closes the seat link on its message (Godot's 1009). That close spends no reconnect attempt, and the same text's
## render is held after it: never sent again. Through all six, within the wait each one has, the
## seat link is never given up, the rows are the loaded text's (its QUICK answered), the prose
## line says the answer closed the link instead of waiting for ever, and the editor's line says
## the answer is too large to read and to shorten the file. A landed load clears the Undo list,
## so the committed playbook is then loaded back, as the planning wire began: the legs after this
## one edit it, Undo takes their edits back to its bytes, and the leg goes on to seal and Ready.
func _closes_on_a_message() -> bool:
	var steps: PackedStringArray = []
	for index in 18000:
		steps.append("{\"hold\":{}}")
	var big := ("{\"schema_version\":{\"major\":1},\"declarative\":{\"route\":[" + ",".join(steps) + "]}}").to_utf8_buffer()
	var closes := _count(_state(), "message_drops_seat")
	for attempt in 6:
		var before := _count(_editor(), "closed_on_calls")
		if not editor.load_bytes(big):
			_failures.append("the planning wire: big load %d was not taken to the gateway" % attempt)
			return false
		if not await _until(func() -> bool: return _count(_editor(), "closed_on_calls") > before and not _editor().get("busy", true), PUSH_WAIT):
			_failures.append("the planning wire: big load %d was not settled on its message: %s %s" % [attempt, _state(), _editor().get("status_key", "")])
			return false
		if String(vista.bridge.watch_link_action(1)) == "give_up":
			_failures.append("the planning wire: big load %d gave the seat link up" % attempt)
			return false
	var dropped := _count(_state(), "message_drops_seat") - closes
	if dropped < 1:
		_failures.append("the planning wire: six big loads never closed the seat link on their message")
	if vista.bridge.watch_link_attempts(1) != 0:
		_failures.append("the planning wire: closes on the player's loads spent the seat link's attempts (%d)" % vista.bridge.watch_link_attempts(1))
	if vista.bridge.editor_bytes() != big:
		_failures.append("the planning wire: a load whose QUICK answer fits did not land")
	var unchecked: Dictionary = _editor().get("unchecked", {})
	if String(unchecked.get("rows", {}).get("key", "?")) != "" or not _editor().get("rows_current", false):
		_failures.append("the planning wire: the loaded text's rows are not its QUICK answer: %s" % [unchecked.get("rows", {})])
	if String(unchecked.get("prose", {}).get("key", "")) != "closed_on" or _editor().get("prose_current", true):
		_failures.append("the planning wire: the render that closed the link is not said on the prose line: %s" % [unchecked.get("prose", {})])
	if String(_editor().get("status_key", "")) != "call_answer_too_large":
		_failures.append("the planning wire: the editor's line after the big loads is `%s`" % _editor().get("status_key", ""))
	print("[watch-check] the planning wire: six loads with no wait, each landing (QUICK fits), %d close(s) on the render's message, the rest held; the seat link kept, no attempt spent; the editor says: `%s`" % [dropped, Strings.text("status_" + String(_editor().get("status_key", "")), {"detail": _editor().get("status_detail", "")})])
	# A load that lands clears the Undo list, so the committed playbook is loaded back, as the
	# planning wire began, checked and rendered: the legs after this one edit it, and Undo takes
	# their edits back to its bytes.
	var committed := FileAccess.get_file_as_bytes(COMMITTED)
	editor.load_file(COMMITTED)
	if not await _editor_until(func(s: Dictionary) -> bool: return vista.bridge.editor_bytes() == committed and _count(s, "undo_depth") == 0 and s.get("rows_current", false) and s.get("prose_current", false) and s.get("rules_editable", false) and not s.get("busy", true), "the planning wire: the committed playbook never came back after the big loads"):
		return false
	return true


## The rule-list chip whose pointer ends in `suffix`, by its index, or -1.
func _chip_at(state: Dictionary, suffix: String) -> int:
	var chips: Array = state.get("rule_list", {}).get("chips", [])
	for index in chips.size():
		if String(chips[index].get("pointer", "")).ends_with(suffix):
			return index
	return -1


## The rule list's button carrying the meta `meta`, or null.
func _rule_button(meta: String) -> Button:
	for button in editor._rules_box.find_children("*", "Button", true, false):
		if (button as Button).has_meta(meta):
			return button
	return null


func _state() -> Dictionary:
	return vista.bridge.watch_state()


func _editor() -> Dictionary:
	return vista.bridge.editor_state()


## Waits until `condition` holds of the editor's state, or records `failure`.
func _editor_until(condition: Callable, failure: String) -> bool:
	if await _until(func() -> bool: return condition.call(_editor()), EDIT_WAIT):
		return true
	_failures.append("%s (editor state: %s)" % [failure, _editor()])
	return false


## Waits, a frame at a time, until `condition` holds or `limit` wall seconds pass.
func _until(condition: Callable, limit: float) -> bool:
	var timer := get_tree().create_timer(limit)
	while not condition.call():
		if timer.time_left <= 0.0 or _link_gone():
			return condition.call()
		await get_tree().process_frame
	return true


func _link_gone() -> bool:
	return not _failures.is_empty() and not _ended


## A count the bridge sent under `key`, read through Values.int_of (S3's `fixp`): one past `int`,
## the bridge's refusal, is this check's failure, said as unreadable, and reads as -1 here.
func _count(answer: Dictionary, key: String) -> int:
	var read: Variant = Values.int_of(answer.get(key, 0))
	if read is int:
		return read
	_failures.append(Values.unreadable_text(key, read))
	return -1


## How long the ended-match leg may take to play its one-seat match to its end.
const ENDED_WAIT := 240.0

## **An ended match is the host's to refuse** (lane `opsave`, carried from S3's `fixq-c`): a
## one-seat match is played to its end, through each recap's Continue, then its line is resumed
## through the real lobby, which ends in the host's refusal in its own words, the chooser back
## and no host left. The host announced and served an ended match before.
func _ended_match_refused() -> bool:
	var line := HostLink.config_line(HostLink.match_id("watch-check-ended"), "1000", 1, CHECK_SEED, CHECK_PRESET)
	if not link.start(_gamectl, _root, line):
		_failures.append("the ended-match leg could not start its host")
		return false
	var pid: int = link.host_pid()
	# One command a phase of a round: Ready in a Lull, Skip in a Push, Continue in a recap.
	var acted := ""
	var timer := get_tree().create_timer(ENDED_WAIT)
	while timer.time_left > 0.0:
		await get_tree().process_frame
		var state := _state()
		var phase := String(state.get("phase", ""))
		if phase == "ended":
			break
		var step := "%s-%s" % [phase, state.get("round", 0)]
		if step == acted:
			continue
		match phase:
			"lull":
				vista.bridge.watch_command("ready", 0)
				acted = step
			"push":
				vista.bridge.watch_command("skip", 0)
				acted = step
			"recap":
				vista.bridge.watch_command("end_recap", 0)
				acted = step
	if String(_state().get("phase", "")) != "ended":
		_failures.append("the ended-match leg's one-seat match did not end in %.0f s: %s" % [ENDED_WAIT, _state()])
		return false
	link.quit()
	if not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
		_failures.append("the ended match's host (pid %d) was still running after its standard input closed" % pid)
		return false
	link.reset()
	print("[watch-check] a one-seat match was played to its end (pid %d); resuming its line" % pid)
	if not await _resume_refused("an ended match", line.strip_edges(), "this match has ended"):
		return false
	var path := ProjectSettings.globalize_path(ENDED_REMEMBERED)
	if FileAccess.file_exists(path) and DirAccess.remove_absolute(path) != OK:
		_failures.append("the ended-match leg could not remove %s" % path)
		return false
	return true


## The private match cache the unsaved-host leg points its host at: a folder of the check's own.
const UNSAVED_DATA := "user://watch_check_data"
## How long the unsaved-host leg may wait for the lobby to say the host stopped: the reconnect
## policy's give-up, with room for a slow frame.
const UNSAVED_WAIT := 90.0

## **A host that cannot write a save says why and is gone** (lane `opsave`, fix pass 2): the
## real lobby starts a new match whose host keeps its private match cache in a folder of the
## check's own (the per-user data variable set for the child), the match's `save.json.partial`
## is made a folder, so no save can be written, and Ready ends the Lull: the host answers with
## its typed error, says why on its standard error and exits, and the lobby shows the host's
## words (what could not be written and what the disk holds), with the chooser back and the
## host process gone.
func _unsaved_host_said() -> bool:
	var data := ProjectSettings.globalize_path(UNSAVED_DATA)
	if DirAccess.make_dir_recursive_absolute(data) != OK:
		_failures.append("the unsaved-host leg could not make %s" % data)
		return false
	var variable := "XDG_DATA_HOME"
	if OS.get_name() == "Windows":
		variable = "LOCALAPPDATA"
	elif OS.get_name() == "macOS":
		variable = "HOME"
	var held := OS.get_environment(variable)
	OS.set_environment(variable, data)
	var lobby: Node = LobbyScene.instantiate()
	lobby.remembered = ENDED_REMEMBERED
	add_child(lobby)
	await get_tree().process_frame
	lobby.new_match()
	# The variable is the child's: set for the spawn and put back at once.
	OS.set_environment(variable, held)
	var ok := await _unsaved_host_lobby(lobby, data)
	var pid: int = lobby.link.host_pid()
	lobby.link.quit()
	if pid >= 0 and not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
		_failures.append("the unsaved-host leg's host (pid %d) was still running" % pid)
		ok = false
	lobby.queue_free()
	await get_tree().process_frame
	var path := ProjectSettings.globalize_path(ENDED_REMEMBERED)
	if FileAccess.file_exists(path) and DirAccess.remove_absolute(path) != OK:
		_failures.append("the unsaved-host leg could not remove %s" % path)
		ok = false
	if not _remove_tree(data):
		_failures.append("the unsaved-host leg could not remove %s" % data)
		ok = false
	return ok


## The unsaved-host leg's lobby, once its new match is starting under `data`.
func _unsaved_host_lobby(lobby: Node, data: String) -> bool:
	var announced := await _until(func() -> bool: return lobby._announced and String(lobby.vista.bridge.watch_state().get("phase", "")) == "lull", START_WAIT)
	if not announced:
		_failures.append("the unsaved-host leg's match never reached its Lull: `%s`" % lobby._status.text)
		return false
	var pid: int = lobby.link.host_pid()
	var match_id := String(lobby._line).get_slice("\t", 0)
	var folder := _folder_named(data, match_id)
	if folder == "":
		_failures.append("the unsaved-host leg found no folder of match %s under %s" % [match_id, data])
		return false
	if DirAccess.make_dir_recursive_absolute(folder.path_join("save.json.partial")) != OK:
		_failures.append("the unsaved-host leg could not block %s's save" % folder)
		return false
	lobby.vista.bridge.watch_command("ready", 0)
	var stopped := await _until(func() -> bool: return not lobby._started and lobby._chooser.visible, UNSAVED_WAIT)
	var said := String(lobby._status.text)
	if not stopped:
		_failures.append("the unsaved-host leg: the lobby did not say the host stopped in %.0f s: `%s`" % [UNSAVED_WAIT, said])
		return false
	if OS.is_process_running(pid):
		_failures.append("the unsaved-host leg: the host (pid %d) is still running after the lobby said it failed" % pid)
		return false
	if not said.contains("save of round 1 could not be written") or not said.contains("this host stops") or not said.contains(Strings.text("link_host_exited", {"words": ""}).strip_edges()):
		_failures.append("the unsaved-host leg: the lobby does not show the host's words: `%s`" % said)
		return false
	print("[watch-check] a host that could not write its save stopped (pid %d), and the lobby said: %s" % [pid, said.strip_edges().replace("\n", " ")])
	return true


## The folder named `name` under `root`, searched to any depth; "" when there is none.
func _folder_named(root: String, name: String) -> String:
	var dir := DirAccess.open(root)
	if dir == null:
		return ""
	for child in dir.get_directories():
		var path := root.path_join(child)
		if child == name:
			return path
		var found := _folder_named(path, name)
		if found != "":
			return found
	return ""


## Remove `path` and everything under it; whether it is gone.
func _remove_tree(path: String) -> bool:
	var dir := DirAccess.open(path)
	if dir == null:
		return not DirAccess.dir_exists_absolute(path)
	for child in dir.get_directories():
		if not _remove_tree(path.path_join(child)):
			return false
	for file in dir.get_files():
		if DirAccess.remove_absolute(path.path_join(file)) != OK:
			return false
	return DirAccess.remove_absolute(path) == OK


## THE PLAYTEST LEG (S3's `prob`, part 2; the plan's E10 and R6). The first host's match is
## played with Playtest mode on: switched on before the first Lull opens, with the camera rig's
## moves counted (the keys and wheel legs make real ones), and the window's focus lost and
## regained in round 1's Push. After the recap the survey is answered and the bundle exported to a
## scratch folder, then read back with Godot's own zip reader (`_playtest_bundle`).
const PLAYTEST_FOLDER := "user://watch_check_playtest"
const PLAYTEST_WORDS := "I wanted to stop the scouts walking past the vent."
const PLAYTEST_LINE := "^[1-9][0-9]* (unknown|lobby|lull|push|recap|ended) [a-z_]+( [A-Z0-9_]+)? t=(0|[1-9][0-9]*)$"


## Playtest mode on, before anything is read, and the camera rig's moves forwarded as the lobby's
## `playtest.gd` forwards them.
func _playtest_on() -> void:
	if not vista.bridge.playtest_set(true, true):
		_failures.append("Playtest mode did not switch on")
	vista.rig.moved.connect(func(kind: String) -> void: vista.bridge.playtest_camera(kind))


## The survey answered, the bundle exported to a scratch folder and read back: exactly one file,
## the four entries, round 1's Push split by focus, its Lull ended by Ready, the camera's moves,
## the survey's words as written, every log line in its grammar, and no match id, token, port,
## notebook text, path or user name in any byte of it.
func _playtest_bundle() -> bool:
	var folder := ProjectSettings.globalize_path(PLAYTEST_FOLDER)
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		_failures.append("the Playtest leg could not make %s" % folder)
		return false
	for old in DirAccess.get_files_at(folder):
		if DirAccess.remove_absolute(folder + "/" + old) != OK:
			_failures.append("the Playtest leg could not clear %s" % old)
			return false
	var path := folder + "/pharmakos-playtest.zip"
	# The survey goes with the export, as answered at this moment.
	var written: Dictionary = vista.bridge.playtest_export(path, "yes", "partly", PLAYTEST_WORDS)
	if not written.get("written", false):
		_failures.append("Export playtest bundle did not write: %s" % written)
		return false
	if Array(DirAccess.get_files_at(folder)) != ["pharmakos-playtest.zip"]:
		_failures.append("the export did not write exactly one file: %s" % [DirAccess.get_files_at(folder)])
	var bytes := FileAccess.get_file_as_bytes(path)
	var reader := ZIPReader.new()
	if reader.open(path) != OK:
		_failures.append("Godot's zip reader cannot open the bundle")
		return false
	var entries: PackedStringArray = reader.get_files()
	var texts := {}
	for entry in entries:
		texts[entry] = reader.read_file(entry).get_string_from_utf8()
	reader.close()
	if entries != PackedStringArray(["manifest.json", "survey.json", "metrics.json", "diagnostics.log"]):
		_failures.append("the bundle's entries are %s" % entries)
		return false
	var manifest: Dictionary = JSON.parse_string(texts["manifest.json"])
	var metrics: Dictionary = JSON.parse_string(texts["metrics.json"])
	var survey: Dictionary = JSON.parse_string(texts["survey.json"])
	# One match, the one under way: its preset as the gateway named it, its ordinal a count.
	var matches: Array = manifest.get("matches", [])
	if int(manifest.get("bundle_format", 0)) != 1 or matches.size() != 1 or String(matches[0].get("preset", "")) != CHECK_PRESET or int(matches[0].get("match", 0)) != 1 or String(matches[0].get("ended", "")) != "under_way":
		_failures.append("the manifest is not format 1 of one %s match under way: %s" % [CHECK_PRESET, manifest])
	var pushes: Array = metrics.get("push_time", [])
	var lulls: Array = metrics.get("lull_time", [])
	if int(metrics.get("pushes", 0)) != 1 or int(metrics.get("skipped_pushes", -1)) != 0 or pushes.size() != 1:
		_failures.append("round 1's Push is not the one Push recorded: %s" % metrics)
	elif int(pushes[0].get("round", 0)) != 1 or int(pushes[0].get("focused_us", 0)) <= 0 or int(pushes[0].get("unfocused_us", 0)) <= 0:
		_failures.append("round 1's Push is not split by the focus it lost and regained: %s" % pushes[0])
	if lulls.size() != 1 or String(lulls[0].get("ended_by", "")) != "ready" or int(lulls[0].get("used_us", 0)) <= 0:
		_failures.append("round 1's Lull is not recorded as ended by Ready: %s" % [lulls])
	if int(metrics.get("speed_changes", {}).get("4", 0)) < 1:
		_failures.append("the 4x the check chose is not counted: %s" % metrics.get("speed_changes"))
	var camera: Dictionary = metrics.get("camera_moves", {})
	if int(camera.get("pan", 0)) < 1 or int(camera.get("zoom", 0)) < 1 or int(camera.get("follow", 0)) < 1:
		_failures.append("the camera legs' moves are not counted: %s" % camera)
	if String(survey.get("wanted_to_change", {}).get("text", "")) != PLAYTEST_WORDS or String(survey.get("understood_why", "")) != "partly":
		_failures.append("the survey is not as answered: %s" % survey)
	var shape := RegEx.create_from_string(PLAYTEST_LINE)
	var log_lines: PackedStringArray = String(texts["diagnostics.log"]).strip_edges().split("\n")
	var survey_lines := 0
	for line in log_lines:
		if shape.search(line) == null:
			_failures.append("a diagnostics line off its grammar: `%s`" % line)
		if " survey_answered " in line:
			survey_lines += 1
	if survey_lines != 1:
		_failures.append("the log holds %d survey lines, not the export's one" % survey_lines)
	# The four files' words and the zip's own entry names: what a reader of the bundle can see.
	var all := ("\n".join(entries) + "\n" + "\n".join(texts.values())).to_lower()
	var secrets := [_line.split("\t")[0], String(link._tokens[0]), String(link._tokens[1]), "127.0.0.1:%d" % link._port, ":%d/" % link._port, NOTES, folder, OS.get_environment("USERNAME"), OS.get_environment("USER"), "user://", ":\\", "/home/", "/users/", "appdata"]
	for secret in secrets:
		var needle := String(secret).to_lower()
		if needle.length() >= 4 and all.contains(needle):
			_failures.append("the bundle holds `%s`" % secret)
	if DirAccess.remove_absolute(path) != OK or DirAccess.remove_absolute(folder) != OK:
		_failures.append("the Playtest leg could not remove its bundle")
	print("[watch-check] Playtest: one %d-byte bundle of %s; round 1's Push %s us focused, %s us not; its Lull %s us to Ready; camera %s; %d log lines in grammar; no id, token, port, notebook, path or name in it" % [bytes.size(), ", ".join(entries), pushes[0].get("focused_us", "?") if not pushes.is_empty() else "?", pushes[0].get("unfocused_us", "?") if not pushes.is_empty() else "?", lulls[0].get("used_us", "?") if not lulls.is_empty() else "?", camera, log_lines.size()])
	return _failures.is_empty()
