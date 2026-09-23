# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The headless-driven run against a REAL `gamectl host` (skeleton-plan-t16a-notes.md
# section A (4)): lobby -> end_lull -> 4x -> segment end.
#
# It drives the same scene pieces the lobby does - the host link, the vista, the bridge's
# watch rig - with a script where the human would be, and asserts:
#
#   * the host announced, both connections opened and the first keyframe arrived whole;
#   * NEITHER CONNECTION DROPPED across an idle stretch longer than the gateway's
#     30-second READ_TIMEOUT: in a Lull the seat connection has nothing to ask, so what
#     keeps it alive is the keep-alive, and what keeps the admin connection alive is the
#     host clock it reports four times a second;
#   * `end_lull` opens the Push, the pacer at 4x plays it, and the admin connection's
#     `_status` footer reaches RECAP;
#   * the bridge caught no panic, the gateway refused no call, and entities were drawn;
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
## How long the Push may take to play out at 4x.
const PUSH_WAIT := 240.0
## How long the host may take to exit once its standard input is closed.
const EXIT_WAIT := 15.0
## The segment played: short, so the check is quick. A lobby setting, carried on the
## config line like any other.
const SEGMENT := "30000"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink

var _failures: Array[String] = []
var _refusals: Array[String] = []
var _events := 0
var _ended := false


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	var ladder := SEGMENT
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--segment="):
			ladder = arg.trim_prefix("--segment=")
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	var gamectl := HostLink.find_gamectl()
	print("[watch-check] starting %s" % gamectl)
	var match_id := "watch-check-%d" % OS.get_process_id()
	if not link.start(gamectl, HostLink.find_root(), HostLink.config_line(match_id, ladder)):
		_finish()
		return
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

	vista.bridge.watch_command("speed", 4)
	vista.bridge.watch_command("end_lull", 0)
	if not await _until(func() -> bool: return _state().get("phase", "") == "push", START_WAIT):
		_failures.append("end_lull did not open the Push")
		_finish()
		return
	print("[watch-check] Push begun at 4x")
	var follow: Node3D = vista.my_commander()
	if follow != null:
		vista.rig.follow(follow)
	if not await _until(func() -> bool: return _state().get("phase", "") == "recap", PUSH_WAIT):
		_failures.append("the Push never reached its recap")
	_finish()


func _finish() -> void:
	var state := _state()
	print("[watch-check] final state: %s" % state)
	print("[watch-check] %d event rows; %s" % [_events, vista.bridge.panic_report()])
	if int(state.get("drops_admin", 0)) != 0 or int(state.get("drops_seat", 0)) != 0:
		_failures.append("a connection dropped: admin %s, seat %s" % [state.get("drops_admin"), state.get("drops_seat")])
	if vista.bridge.caught_panics() != 0:
		_failures.append("%d panic(s) were caught at the bridge boundary" % vista.bridge.caught_panics())
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
		vista.take_view(answer["view"])


func _state() -> Dictionary:
	return vista.bridge.watch_state()


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
