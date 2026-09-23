# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The lobby and the watch rig: host a match, watch it, and nothing else a human could use
# to steer it once the Push begins.
#
# The lobby starts the match host (`scripts/host_link.gd`: `gamectl host` as a child
# process, two WebSocket connections), and the watch rig is what spec section 3 names for
# a Push: one camera rig with free-look and a follow-commander toggle, the live event list
# from `get_segment_feed`, 1x, 2x and 4x, and the instant skip to segment end. There is NO
# pause control (decisions-log item 108 (3)): the pacer keeps playing from elapsed time
# whether or not the window has focus.
#
# The Lull ends when every seat is ready or its timer runs out - the bridge's watch rig
# decides that from the host clock's answer and calls `end_lull` itself. Ready is this
# seat's `set_ready`. Continue leaves the recap. Every button is a request to the bridge;
# nothing here computes a time, a price or a rule, and nothing here can show more of the
# map than the gateway sent (AGENTS.md section 3 rule 4).
#
# PLACEHOLDER: the whole layout - a strip of buttons and a text column over the vista -
# is the skeleton's; the editor, the notes box and the real lobby are T19's and S6's.

extends Node

const HostLink := preload("res://scripts/host_link.gd")

## How many event rows the list keeps on screen. PLACEHOLDER, UI, OWNER at S6.
const EVENT_ROWS := 14

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink

var _status: Label
var _events: Label
var _rows: Array[String] = []
var _follow: Button
var _failed := false


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	_build_ui()
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	link.announced.connect(_on_announced)
	var match_id := "local-%d" % OS.get_process_id()
	_status.text = "Starting the match host..."
	link.start(HostLink.find_gamectl(), HostLink.find_root(), HostLink.config_line(match_id, HostLink.LADDER))


func _process(_delta: float) -> void:
	link.pump()
	if _failed:
		return
	var state: Dictionary = vista.bridge.watch_state()
	if state.is_empty():
		return
	var line := "Round %s - %s" % [state.get("round", 0), String(state.get("phase", "")).to_upper()]
	var timer := String(state.get("timer", ""))
	if timer != "":
		line += "  " + timer
	line += "   speed %sx" % state.get("speed", 1)
	if state.get("skipping", false):
		line += "  (skipping)"
	if state.get("all_ready", false):
		line += "  all ready"
	_status.text = line


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST or what == NOTIFICATION_PREDELETE:
		if link != null:
			link.quit()


func _on_announced(_match_id: String) -> void:
	_status.text = "Connected. Waiting for the view..."


func _on_failed(reason: String) -> void:
	_failed = true
	_status.text = "The match host failed: %s" % reason


func _on_received(answer: Dictionary) -> void:
	if answer.has("view"):
		var view: Dictionary = answer["view"]
		var first: bool = vista.marker_count() == 0
		vista.take_view(view)
		if first and view.get("complete", false):
			vista.frame_whole_map()
	if answer.get("phase_changed", false) and answer.get("phase", "") == "push":
		_rows.clear()
	for row in answer.get("events", PackedStringArray()):
		_rows.append(String(row))
	while _rows.size() > EVENT_ROWS:
		_rows.pop_front()
	_events.text = "\n".join(_rows)
	var error := String(answer.get("error", ""))
	if error != "":
		push_warning("[lobby] the gateway refused a call: %s" % error)


func _build_ui() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	var column := VBoxContainer.new()
	column.position = Vector2(12, 12)
	layer.add_child(column)
	_status = Label.new()
	column.add_child(_status)
	var buttons := HBoxContainer.new()
	column.add_child(buttons)
	for speed in [1, 2, 4]:
		_button(buttons, "%dx" % speed, func() -> void: vista.bridge.watch_command("speed", speed))
	_button(buttons, "Skip", func() -> void: vista.bridge.watch_command("skip", 0))
	_button(buttons, "Ready", func() -> void: vista.bridge.watch_command("ready", 0))
	_button(buttons, "Continue", func() -> void: vista.bridge.watch_command("end_recap", 0))
	_follow = _button(buttons, "Follow", _toggle_follow)
	_follow.toggle_mode = true
	_button(buttons, "Whole map", func() -> void: vista.frame_whole_map())
	_events = Label.new()
	column.add_child(_events)


func _button(parent: Node, text: String, pressed: Callable) -> Button:
	var button := Button.new()
	button.text = text
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(pressed)
	parent.add_child(button)
	return button


func _toggle_follow() -> void:
	if _follow.button_pressed:
		vista.rig.follow(vista.my_commander())
	else:
		vista.rig.follow(null)
