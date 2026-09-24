# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The headless-driven run against a REAL `gamectl host` (skeleton-plan-t16a-notes.md
# section A (4); T19's acceptance, skeleton-plan-w6-notes.md section A4): lobby -> the
# editor -> Ready ends the Lull -> 4x -> segment end.
#
# It drives the same scene pieces the lobby does - the host link, the vista, the editor,
# the bridge's watch rig - with a script where the human would be, and asserts:
#
#   * the host announced, both connections opened and the first keyframe arrived whole;
#   * NEITHER CONNECTION DROPPED across an idle stretch longer than the gateway's
#     30-second READ_TIMEOUT: in a Lull the seat connection has nothing to ask, so what
#     keeps it alive is the keep-alive, and what keeps the admin connection alive is the
#     host clock it reports four times a second;
#   * THE EDITOR (T19, pull request 1), on the same seat connection:
#       - Load REFUSES an out-of-vocabulary file (`fixtures/out_of_vocabulary.json`) with
#         the verifier's code and JSON Pointer, opens nothing, and strips nothing; the
#         refusal's row has its plain sentence as its accessible name;
#       - a file with a machine-applicable diagnostic (`fixtures/needs_a_fix.jsonc`) is
#         fixed by its Fix button, through `patch_plan` with the verifier's own patch;
#       - a committed playbook in plan-core's canonical form, `fixtures/editor_check.jsonc`,
#         opens, QUICK qualifies it, and it saves back BYTE FOR BYTE;
#       - a placement ghost comes back with QUICK's verdict, the placement is taken and
#         undone, and the undo gives the loaded bytes back;
#       - one map action (Alt-click on the seat's own beacon, Go here, nearest own beacon)
#         is applied through `patch_plan`, QUICK runs, the route is priced, and FULL runs
#         after 600 ms idle and qualifies;
#       - the notebook is saved through `save_notes` and a draft through `save_draft`;
#       - the playbook is submitted and accepted, the SUBMITTED BYTES EQUAL
#         `fixtures/editor_check.expected.jsonc` (plan-core's own patch of its canonical
#         form: see that file's note in godot/README.md), and `gamectl verify` accepts the
#         file the editor saved;
#   * Ready (`set_ready`) ends the Lull: the host is a one-seat match, so this seat's Ready
#     is every seat's, the host clock's answer says `all_ready`, and the admin connection
#     calls `end_lull` (skeleton-plan-t16a-notes.md section B, "T19" (6));
#   * the pacer at 4x plays the Push, and the admin connection's `_status` footer reaches
#     RECAP; the seat connection then catches up before the rows are counted;
#   * the bridge caught no panic, refused no view page, the gateway refused no call - the
#     editor's included, so no burst of edits met RATE_LIMITED - and entities were drawn;
#   * the child exits once the client lets go of its standard input.
#
# The last line printed before a pass carries the host's process id, so the CI step can
# also check from outside that no `gamectl` is left once Godot has exited.
#
# Run headless, after `cargo xtask stage-client` and a debug build of gamectl:
#
#     godot --headless --path godot res://scenes/watch_check.tscn -- \
#         --gamectl=<target>/debug/gamectl --root=<repository> [--segment=30000]
#
# Exit codes: 0 when everything above held, 1 when anything did not.

extends Node

const HostLink := preload("res://scripts/host_link.gd")

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
## How long the host may take to exit once its standard input is closed.
const EXIT_WAIT := 15.0
## The segment played: short, so the check is quick. A lobby setting, carried on the
## config line like any other.
const SEGMENT := "30000"
## One seat, so that this seat's Ready is every seat's and Ready ends the Lull through
## `all_ready`, as the lobby's does.
const CHECK_SEATS := 1

## The committed playbook the editor opens, in plan-core's canonical form.
const COMMITTED := "res://fixtures/editor_check.jsonc"
const OUT_OF_VOCABULARY := "res://fixtures/out_of_vocabulary.json"
const NEEDS_A_FIX := "res://fixtures/needs_a_fix.jsonc"
const EXPECTED := "res://fixtures/editor_check.expected.jsonc"
## Where the check saves the player's file, as a player would.
const SAVED := "user://editor_check.jsonc"
const NOTES := "watch check"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink
@onready var editor: CanvasLayer = $Editor

var _failures: Array[String] = []
var _refusals: Array[String] = []
var _events := 0
var _ended := false
var _gamectl := ""
var _root := ""


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	var ladder := SEGMENT
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--segment="):
			ladder = arg.trim_prefix("--segment=")
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	editor.setup(vista)
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	_gamectl = HostLink.find_gamectl()
	_root = HostLink.find_root()
	print("[watch-check] starting %s" % _gamectl)
	var match_id := "watch-check-%d" % OS.get_process_id()
	if not link.start(_gamectl, _root, HostLink.config_line(match_id, ladder, CHECK_SEATS)):
		_finish()
		return
	editor.begin(HostLink.my_seat())
	print("[watch-check] host pid %d" % link.host_pid())
	_run()


func _process(_delta: float) -> void:
	if not _ended:
		link.pump()


func _run() -> void:
	if not await _until(func() -> bool: return _state().get("phase", "") == "lull" and _state().get("view_settled", false), START_WAIT):
		_failures.append("the host never reached a Lull with a complete keyframe")
		_finish()
		return
	print("[watch-check] Lull reached; %d markers drawn; idling %.0f s" % [vista.marker_count(), IDLE_WAIT])
	await get_tree().create_timer(IDLE_WAIT).timeout
	var idle := _state()
	print("[watch-check] after the idle stretch: %s" % idle)
	if idle.get("phase", "") != "lull":
		_failures.append("the Lull ended by itself during the idle stretch")
		_finish()
		return

	if not await _edit():
		_finish()
		return

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
	if not await _until(func() -> bool: return _state().get("phase", "") == "recap", PUSH_WAIT):
		_failures.append("the Push never reached its recap")
		_finish()
		return
	if not await _until(func() -> bool: return _state().get("seat_settled", false), START_WAIT):
		_failures.append("the seat connection never caught up after the recap began")
	_finish()


## The editor's half of the run. Returns false when a step failed, with the reason recorded.
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
	vista.bridge.editor_preview_place(spot)
	if not await _editor_until(func(s: Dictionary) -> bool: return s.get("ghost", {}).get("state", "waiting") != "waiting", "the placement ghost never came back"):
		return false
	print("[watch-check] ghost at %s: %s %s" % [spot, _editor()["ghost"].get("state"), _editor()["ghost"].get("sentence")])
	if not editor.act("place", {"voxel": spot}) or int(_editor().get("revision", 0)) != loaded_revision + 1:
		_failures.append("the checked placement was not taken at once")
		return false
	vista.bridge.editor_undo()
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) == loaded_revision + 2 and not s.get("busy", true), "Undo never came back"):
		return false
	if vista.bridge.editor_bytes() != original:
		_failures.append("Undo did not give the loaded bytes back")
	print("[watch-check] placement taken and undone; the bytes are the loaded ones again")

	# 5. One map action: Alt-click on the seat's own beacon, Go here, the nearest own beacon.
	var own := ""
	for beacon in vista.beacons():
		if beacon["owner"] == vista.my_seat:
			own = String(beacon["id"])
	if own == "":
		_failures.append("the view shows no beacon of this seat's")
		return false
	vista.bridge.editor_describe_beacon(own)
	if not await _editor_until(func(s: Dictionary) -> bool: return String(s.get("beacon_prose", "")) != "", "get_beacon never described %s" % own):
		return false
	var before := int(_editor().get("revision", 0))
	if not editor.act("go", {"selector": "nearest"}):
		_failures.append("the map action was not taken")
		return false
	if not await _editor_until(func(s: Dictionary) -> bool: return int(s.get("revision", 0)) > before and s.get("verdict", "") == "full" and s.get("route", {}).get("current", false), "the map action was never patched, checked and priced"):
		return false
	var after := _editor()
	if not after.get("qualifies", false):
		_failures.append("FULL did not qualify the edited playbook: %s" % [after.get("rows", [])])
	var route: Dictionary = after.get("route", {})
	print("[watch-check] map action applied; FULL qualifies it; route %s point(s), legs %s" % [(route.get("points", []) as Array).size(), route.get("legs")])
	if (route.get("points", []) as Array).size() < 2 or not route.get("reachable", false):
		_failures.append("the edited route was not priced: %s" % [route])

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


func _finish() -> void:
	var state := _state()
	print("[watch-check] final state: %s" % state)
	print("[watch-check] %d event rows; %s" % [_events, vista.bridge.panic_report()])
	if int(state.get("drops_admin", 0)) != 0 or int(state.get("drops_seat", 0)) != 0:
		_failures.append("a connection dropped: admin %s, seat %s" % [state.get("drops_admin"), state.get("drops_seat")])
	if vista.bridge.caught_panics() != 0:
		_failures.append("%d panic(s) were caught at the bridge boundary" % vista.bridge.caught_panics())
	if int(state.get("view_refusals", 0)) != 0:
		_failures.append("the bridge refused %s view page(s)" % state.get("view_refusals"))
	if not _refusals.is_empty():
		_failures.append("the gateway refused %d call(s): %s" % [_refusals.size(), "; ".join(_refusals)])
	if vista.marker_count() == 0:
		_failures.append("no entity was drawn")
	_ended = true
	var pid: int = link.host_pid()
	link.quit()
	if pid >= 0:
		if not await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT):
			_failures.append("the host (pid %d) was still running after its standard input closed" % pid)
		else:
			print("[watch-check] the host exited once its standard input closed")
	if _failures.is_empty():
		print("[watch-check] OK (host pid %d)" % pid)
		get_tree().quit(0)
	else:
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
