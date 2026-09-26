# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The headless-driven run against a REAL `gamectl host` (skeleton-plan-t16a-notes.md
# section A (4); T19's acceptance, skeleton-plan-w6-notes.md section A4 as decisions-log
# item 112 amends it): lobby -> the editor -> the wizard -> Ready ends the Lull -> 4x ->
# recap -> round 2 -> quit in the Lull -> a fresh client resumes it.
#
# It drives the same scene pieces the lobby does - the host link, the vista, the editor,
# the bridge's watch rig - with a script where the human would be, and asserts:
#
#   * the host announced, both connections opened and the first keyframe arrived whole;
#   * NEITHER CONNECTION DROPPED across an idle stretch longer than the gateway's
#     30-second READ_TIMEOUT: in a Lull the seat connection has nothing to ask, so what
#     keeps it alive is the keep-alive, and what keeps the admin connection alive is the
#     host clock it reports four times a second. The stretch is spent in round 2's Lull,
#     so round 1's 180-second Lull timer is left whole for the editing below;
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
#         `gamectl verify` is run on it: exit 0 for a file that places nothing; for one that
#         places a beacon, its only errors must be E0403 at the placed sites, because
#         `gamectl verify` checks against its reference seat view, not this match
#         (crates/gamectl/src/seat.rs), and the live FULL at submit is that file's
#         qualifying check;
#   * THE PIN: Hold & Build instantiated with every parameter
#     `fixtures/instantiate_suggested.json` lists typed explicitly (read from the fixture,
#     never written here) is the fixture's `playbook_jsonc` byte for byte. An explicit value
#     beats a suggestion, so the live operator cannot move it. It is instantiated only,
#     never used or submitted: its anchor is off the map, so QUICK would say E0402;
#   * Ready (`set_ready`) ends the Lull: the host is a two-seat match whose other seat the
#     built-in operator plays and readies at every Lull's start (T18), so this seat's Ready
#     completes every seat's, the host clock's answer says `all_ready`, and the admin
#     connection calls `end_lull` (skeleton-plan-t16a-notes.md section B, "T19" (6));
#   * the pacer at 4x plays the Push, and the admin connection's `_status` footer reaches
#     RECAP; the seat connection then catches up before the rows are counted;
#   * THE RESUME LEG: Continue leads into round 2's Lull, where the carried draft is opened
#     through `get_draft`; the host's standard input is closed (the host saves the Lull)
#     and the host exits; then the scene is changed, so the bridge, its rig and its editor
#     are rebuilt with nothing carried but the config line and the expectations (in
#     Engine metadata), and a new host is started with the same six fields plus `resume`.
#     Round 2's Lull comes back, the notebook comes back through `get_briefing`, the drafts
#     are listed, and the carried draft is opened through `get_draft` with the bytes of
#     round 1's sealed submission;
#   * across both hosts: the bridge caught no panic, refused no view page, the gateway
#     refused no call (so no burst met RATE_LIMITED), entities were drawn, and both hosts
#     exited once the client let go of their standard input.
#
# THE MATCH ID is `watch-check-<unix seconds>-<pid>`, through host_link.gd's one helper
# (decisions-log item 113 (13)): the host keeps its save in the real per-user private match
# cache, and a later local run that reused a pid would be refused. Each local run therefore
# leaves one match folder there that the client cannot remove (godot/README.md). The check
# never touches the lobby's remembered line.
#
# The last line printed before a pass carries both hosts' process ids, so the CI step can
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
## Where the check saves the player's file, as a player would.
const SAVED := "user://editor_check.jsonc"
const NOTES := "watch check"
## What the first scene hands the second (Engine metadata, which survives a scene change).
const CARRY := "pharmakos_watch_check"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink
@onready var editor: CanvasLayer = $Editor

var _failures: Array[String] = []
var _refusals: Array[String] = []
var _events := 0
var _ended := false
var _gamectl := ""
var _root := ""
var _line := ""
var _resumed := false


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	_resumed = Engine.has_meta(CARRY)
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	editor.setup(vista)
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	_gamectl = HostLink.find_gamectl()
	_root = HostLink.find_root()
	var written := ""
	if _resumed:
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
		_line = HostLink.config_line(HostLink.match_id("watch-check"), ladder, CHECK_SEATS)
		written = _line
		print("[watch-check] starting %s" % _gamectl)
	if not link.start(_gamectl, _root, written):
		_finish()
		return
	editor.begin(HostLink.my_seat())
	print("[watch-check] host pid %d" % link.host_pid())
	if _resumed:
		_run_resumed()
	else:
		_run()


func _process(_delta: float) -> void:
	if not _ended:
		link.pump()


func _run() -> void:
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and _state().get("view_settled", false), START_WAIT):
		_failures.append("the host never reached a Lull with a complete keyframe")
		_finish()
		return
	print("[watch-check] Lull reached; %d markers drawn" % vista.marker_count())

	if not await _edit():
		_finish()
		return
	if not await _meter("lull"):
		_finish()
		return
	if not await _templates():
		_finish()
		return
	if not await _pin():
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
	var follow: Node3D = vista.my_commander()
	if follow != null:
		vista.rig.follow(follow)
	if not await _meter("push"):
		_finish()
		return
	if not await _until(func() -> bool: return _state().get("phase", "") == "recap", PUSH_WAIT):
		_failures.append("the Push never reached its recap")
		_finish()
		return
	if not await _until(func() -> bool: return _state().get("seat_settled", false), START_WAIT):
		_failures.append("the seat connection never caught up after the recap began")
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
	if idle.get("phase", "") != "lull" or int(idle.get("drops_admin", 0)) != 0 or int(idle.get("drops_seat", 0)) != 0:
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


## The carried draft is opened (through `get_draft`, the editor's one way to open it), with
## `sealed`'s bytes.
func _carried(sealed: PackedByteArray, where: String) -> bool:
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("status_key", "")) == "carried" and not s.get("busy", true) and s.get("rows_current", false), "%s: the carried draft was never opened" % where):
		return false
	var opened: PackedByteArray = vista.bridge.editor_bytes()
	if opened != sealed:
		_failures.append("%s: the carried draft is not round 1's sealed submission (%d bytes against %d)" % [where, opened.size(), sealed.size()])
		return false
	if int(_editor().get("refusals", 0)) != 0:
		_failures.append("%s: the gateway refused %s of the editor's calls" % [where, _editor().get("refusals")])
	return true


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
	(first.get_meta("send") as Button).pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("wizard", {}).get("current", false) and not bool(_page_of(s.get("wizard", {}), pointer).get("suggested", true)), "%s: the explicit value never came back" % id):
		return false
	editor.refresh()
	var resent: Control = editor.wizard_page_controls()[0]
	if (resent.get_meta("mark") as Label).visible:
		_failures.append("%s: the re-sent page still carries the operator's mark" % id)
	if (resent.get_meta("field") as LineEdit).text != shown:
		_failures.append("%s: the re-sent page shows `%s`, not what was sent, `%s`" % [id, (resent.get_meta("field") as LineEdit).text, shown])

	# Use, through its own button: the gateway's playbook, byte for byte, as one edit.
	var text := String(_editor()["wizard"].get("playbook_jsonc", ""))
	var before := int(_editor().get("revision", 0))
	editor.refresh()
	var use: Button = editor.wizard_use_button()
	if use == null or use.disabled:
		_failures.append("%s: Use is not offered on a current wizard" % id)
		return false
	use.pressed.emit()
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) > before and not s.get("busy", true), "%s: Use never put the playbook in" % id):
		return false
	if vista.bridge.editor_bytes() != text.to_utf8_buffer():
		_failures.append("%s: the editor's text is not the gateway's playbook_jsonc byte for byte" % id)
		return false

	# One map action through patch_plan: Alt-click the seat's own beacon, Go here.
	var own := {}
	for beacon in vista.beacons():
		if beacon["owner"] == vista.my_seat:
			own = beacon
	if own.is_empty():
		_failures.append("the view shows no beacon of this seat's")
		return false
	var used := int(_editor().get("revision", 0))
	editor._on_beacon_clicked(own, true, Vector2.ZERO)
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) > used and s.get("verdict", "") == "quick" and s.get("rows_current", false), "%s: the map action was never patched and checked" % id):
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
	_verify(id, saved, edited.get_string_from_utf8())
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


## `gamectl verify` on the saved file: exit 0 when it places nothing; when it places a beacon,
## the reference seat view's spheres will not hold this match's site, so its only errors
## must be E0403 at the placed sites (the live FULL at submit is that file's check).
func _verify(id: String, saved: String, text: String) -> void:
	var output := []
	var code := OS.execute(_gamectl, PackedStringArray(["--root", _root, "verify", ProjectSettings.globalize_path(saved)]), output, true)
	var report := "".join(output)
	var errors: PackedStringArray = []
	for line in report.split("\n"):
		var words := line.strip_edges().split(" ", false)
		if words.size() >= 3 and words[0] == "error":
			errors.append("%s %s" % [words[1], words[2]])
	var places := "\"place_beacon\"" in text
	if not places:
		if code != 0:
			_failures.append("%s: gamectl verify refused a file that places nothing (exit %d): %s" % [id, code, report])
	else:
		for error in errors:
			if not (error.begins_with("E0403 ") and "/place_beacon/" in error):
				_failures.append("%s: gamectl verify found more than E0403 at the placed sites: %s" % [id, error])
		if code != 0 and errors.is_empty():
			_failures.append("%s: gamectl verify failed with no error listed (exit %d): %s" % [id, code, report])
	print("[watch-check] %s: submitted and accepted; the bytes are the gateway's last text; gamectl verify exit %d%s" % [id, code, (" (errors: %s)" % ", ".join(errors)) if not errors.is_empty() else ""])


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

	# 2. A Fix button applies the verifier's own patch through patch_plan.
	editor.load_file(NEEDS_A_FIX)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("has_text", false) and s.get("rows_current", false) and not s.get("busy", true), "the file with a Fix never opened"):
		return false
	var rows: Array = _editor().get("rows", [])
	var unfixed := int(_editor().get("revision", 0))
	var fixed := false
	for index in rows.size():
		if rows[index].get("code", "") == "E0108" and not (rows[index].get("fixes", []) as Array).is_empty():
			fixed = editor.fix(index, 0)
	if not fixed:
		_failures.append("no Fix button was offered for E0108: %s" % [rows])
		return false
	_check_accessible_names("the Fix rows")
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) > unfixed and s.get("rows_current", false) and s.get("qualifies", false), "the Fix did not leave a playbook QUICK accepts"):
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
	if not editor.save_file(SAVED) or FileAccess.get_file_as_bytes(SAVED) != original:
		_failures.append("opening and saving the committed playbook did not give its bytes back")
	print("[watch-check] opened and saved %s byte for byte" % COMMITTED)
	var loaded_revision := int(_editor().get("revision", 0))

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
	if int(_editor().get("revision", 0)) != loaded_revision + 1:
		_failures.append("the checked placement was not taken at once")
		return false
	vista.bridge.editor_undo()
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) == loaded_revision + 2 and not s.get("busy", true), "Undo never came back"):
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
	var before := int(_editor().get("revision", 0))
	# Through the map surface's own handlers: an Alt-click on the beacon (which also asks
	# get_beacon about it), whose menu's first target is the nearest own beacon, then Go here.
	editor._on_beacon_clicked(own, true, Vector2.ZERO)
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("beacon_prose", "")) != "", "get_beacon never described %s" % own["id"]):
		return false
	editor._on_menu(editor.ITEM_GO)
	editor.close_menu()
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) > before and s.get("verdict", "") == "full" and s.get("route", {}).get("current", false), "the map action was never patched, checked and priced"):
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
func _check_bridge(who: String) -> void:
	var state := _state()
	print("[watch-check] %s's final state: %s" % [who, state])
	print("[watch-check] %s: %d event rows; %s" % [who, _events, vista.bridge.panic_report()])
	if int(state.get("drops_admin", 0)) != 0 or int(state.get("drops_seat", 0)) != 0:
		_failures.append("%s: a connection dropped: admin %s, seat %s" % [who, state.get("drops_admin"), state.get("drops_seat")])
	if vista.bridge.caught_panics() != 0:
		_failures.append("%s: %d panic(s) were caught at the bridge boundary" % [who, vista.bridge.caught_panics()])
	if int(state.get("view_refusals", 0)) != 0:
		_failures.append("%s: the bridge refused %s view page(s)" % [who, state.get("view_refusals")])
	if not _refusals.is_empty():
		_failures.append("%s: the gateway refused %d call(s): %s" % [who, _refusals.size(), "; ".join(_refusals)])
	if vista.marker_count() == 0:
		_failures.append("%s: no entity was drawn" % who)


func _finish() -> void:
	_check_bridge("the resumed client" if _resumed else "the first client")
	_ended = true
	var pid: int = link.host_pid()
	link.quit()
	if pid >= 0:
		if not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
			_failures.append("the host (pid %d) was still running after its standard input closed" % pid)
		else:
			print("[watch-check] the host exited once its standard input closed")
	if _failures.is_empty() and _resumed:
		var first := int((Engine.get_meta(CARRY) as Dictionary).get("first_pid", -1))
		print("[watch-check] OK (host pids %d and %d)" % [first, pid])
		get_tree().quit(0)
	elif _failures.is_empty():
		# A run that stops before the resume leg is not a pass.
		_failures.append("the check ended before its resume leg")
		_report_failures()
	else:
		_report_failures()


func _report_failures() -> void:
	for failure in _failures:
		printerr("[watch-check] FAILED: %s" % failure)
	get_tree().quit(1)


func _on_failed(reason: String) -> void:
	_failures.append("the host link failed: %s" % reason)


func _on_received(answer: Dictionary) -> void:
	var refusal := String(answer.get("error", ""))
	if refusal != "":
		_refusals.append(refusal)
	_events += (answer.get("events", PackedStringArray()) as PackedStringArray).size()
	if answer.has("view"):
		if (answer["view"] as Dictionary).is_empty():
			_failures.append("a get_view page did not decode (the reason is in the log)")
		else:
			vista.take_view(answer["view"])


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
