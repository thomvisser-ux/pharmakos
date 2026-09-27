# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The shipped smoke check (decisions-log item 117 (7)): "reaches the lobby", proved through
# the real lobby rather than beside it. It is the one check the export keeps, reached
# through `boot.gd`'s `--scene=`:
#
#     Pharmakos.exe --headless -- --scene=res://scenes/smoke_check.tscn
#     godot --headless --path <project> -- --scene=res://scenes/smoke_check.tscn \
#         --gamectl=<gamectl> --root=<root>
#
# In order, each wait bounded:
#   1. instance the real lobby and let it run idle for IDLE_WAITS frames, as a player does
#      before choosing: the lobby pumps its host link every frame, and a check that pressed
#      New match in the lobby's first frame would pass over the idle-lobby crash item
#      117 (3) fixed;
#   2. open the credits overlay through the chooser's own Credits button, check it covers
#      the chooser, close it through its own Back, and check the chooser is back - so the
#      release VM runs that script too;
#   3. start a New match through the chooser's own button (never a second spawn, never a
#      bridge call around the lobby) and wait, up to START_WAIT, for the host's announce and
#      for the watch rig's phase to read `lull` (it reads `unknown` long before);
#   4. check the bridge is the real class and has caught no panic, as the client check
#      does;
#   5. end through the lobby's own path: read the host's pid from the link, free the lobby
#      (queued, so its PREDELETE closes the host's standard input at the frame's end), and
#      wait, up to EXIT_WAIT, until that process has exited.
# Only then does it print exactly one `[smoke] OK` line, or one `[smoke] FAIL <reason>`
# line, and exit 0 or 1. Neither line carries a token or any part of the announce line
# (decisions-log item 107 (2)); the host's pid is printed, so a job can check from outside
# that no `gamectl` is left.
#
# Like the watch check it is a check, so it gains no line in AGENTS.md section 3 rule 4
# (item 114 (2)); its `create_timer` waits join the watch check's clock allowance in
# `crates/client-gdext/tests/no_arithmetic.rs`, and it reads no other clock.

extends Node

const Strings := preload("res://scripts/strings.gd")
const LobbyScene := preload("res://scenes/lobby.tscn")

## Frames the lobby runs idle before anything is pressed, and between the overlay's steps.
const IDLE_WAITS := 10
## How long the host may take to generate its map, announce itself and reach the Lull.
const START_WAIT := 180.0
## How long the host may take to exit once the lobby has closed its standard input.
const EXIT_WAIT := 30.0
## The bridge class the extension registers.
const BRIDGE_CLASS := "PharmakosBridge"

var _lobby: Node = null
var _announced := false
var _failed_reason := ""


func _ready() -> void:
	_lobby = LobbyScene.instantiate()
	add_child(_lobby)
	_lobby.link.announced.connect(func(_id: String) -> void: _announced = true)
	_lobby.link.failed.connect(func(reason: String) -> void: _failed_reason = reason)
	await _idle()

	# 2. The credits overlay, through the chooser's own buttons.
	var about := _chooser_button(Strings.text("lobby_about"))
	if about == null:
		_fail("the chooser has no Credits button")
		return
	about.pressed.emit()
	await _idle()
	var overlay: Node = _lobby.get_node_or_null("About")
	if overlay == null or _lobby._chooser.visible:
		_fail("the credits overlay did not open over the chooser")
		return
	overlay.back.pressed.emit()
	await _idle()
	if _lobby.get_node_or_null("About") != null or not _lobby._chooser.visible:
		_fail("Back did not close the credits overlay and give the chooser back")
		return

	# 3. A New match, through the chooser's own button.
	var start := _chooser_button(Strings.text("lobby_new_match"))
	if start == null:
		_fail("the chooser has no New match button")
		return
	start.pressed.emit()
	var reached := await _until(func() -> bool: return _announced and _phase() == "lull", START_WAIT)
	if not reached:
		var why := _failed_reason if _failed_reason != "" else "no announce or no Lull"
		_fail("the match never reached its Lull: %s (phase %s)" % [why, _phase()])
		return

	# 4. The bridge is the real class, and it caught no panic.
	var bridge: Object = _lobby.vista.bridge
	if not ClassDB.class_exists(BRIDGE_CLASS) or bridge.get_class() != BRIDGE_CLASS:
		_fail("the bridge is not a %s: the extension did not load" % BRIDGE_CLASS)
		return
	var caught: int = bridge.caught_panics()
	if caught != 0:
		_fail("the bridge caught %d panic(s)" % caught)
		return

	# 5. End through the lobby's own path.
	var pid: int = _lobby.link.host_pid()
	_lobby.queue_free()
	_lobby = null
	if pid < 0:
		_fail("the lobby never started a host")
		return
	var exited := await _until(func() -> bool: return not OS.is_process_running(pid), EXIT_WAIT)
	if not exited:
		_fail("the host (pid %d) was still running after the lobby closed its standard input" % pid)
		return
	print("[smoke] OK (host pid %d exited)" % pid)
	get_tree().quit(0)


## The chooser's button with this label, or null.
func _chooser_button(label: String) -> Button:
	for child in _lobby._chooser.get_children():
		if child is Button and (child as Button).text == label:
			return child
	return null


## The watch rig's phase, as the bridge reports it.
func _phase() -> String:
	return String(_lobby.vista.bridge.watch_state().get("phase", ""))


## IDLE_WAITS frames, one at a time.
func _idle() -> void:
	for _i in IDLE_WAITS:
		await get_tree().process_frame


## Waits, one frame at a time, until `condition` holds or `limit` wall seconds pass, or the
## host link has failed.
func _until(condition: Callable, limit: float) -> bool:
	var timer := get_tree().create_timer(limit)
	while not condition.call():
		if timer.time_left <= 0.0 or _failed_reason != "":
			return condition.call()
		await get_tree().process_frame
	return true


func _fail(reason: String) -> void:
	print("[smoke] FAIL %s" % reason)
	if _lobby != null:
		_lobby.queue_free()
		_lobby = null
	get_tree().quit(1)
