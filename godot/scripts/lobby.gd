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
# The playbook editor (`scripts/editor.gd`, T19) sits beside the watch rig: its panel on the
# right, its route and ghost on the map. It shares the seat connection, and the bridge fits
# its calls into the seat's rate budget beside the vista's polls.
#
# NEW MATCH OR RESUME (T19 pull request 2; decisions-log items 111 (C9), 112 (6) and 113
# (11)). Nothing starts until the player chooses:
#   * New match names the match through host_link.gd's one helper, `local-<unix
#     seconds>-<pid>`, so it cannot collide with an older match whose save is still in the
#     private match cache, and writes the six-field config line;
#   * Resume last match, shown only when a line is remembered, writes that same line plus
#     `resume`. The gateway checks it against the save and decides where the match goes on
#     (the Push a sealed save began, or the Lull a quit left); a refusal shows the host's
#     own standard-error text, and nothing is retried with other values.
# Once the host has announced, the lobby remembers the six-field line it wrote, in ONE
# `user://` file that holds no token (the tokens stay in host_link.gd's variables).
# Remembering the whole line rather than the id alone means a resume repeats exactly what
# the save was made with, even if a constant in host_link.gd changes between builds. When
# the match ends, the lobby forgets it: an ended match keeps its save and its id stays
# taken (item 113 (11)), so a finished match's last Push is not replayed from here.
#
# PLACEHOLDER: `user://`'s remembered match is a per-machine convenience, not state: the
# save itself is the gateway's, in the private match cache. OWNER, with the save browser,
# S6. PLACEHOLDER: forgetting an ended match (item 113 (11)), and the New/Resume layout,
# OWNER, S6. That layout includes what a refused Resume leaves: the refusal is shown, the
# chooser stays hidden and the line stays remembered, so the player restarts the client to
# pick New match, and the next launch offers the same Resume again. Nothing is retried.
#
# PLACEHOLDER: the whole layout - a strip of buttons and a text column over the vista -
# is the skeleton's; the real lobby is S6's.

extends Node

const HostLink := preload("res://scripts/host_link.gd")
const Strings := preload("res://scripts/strings.gd")

## How many event rows the list keeps on screen. PLACEHOLDER, UI, OWNER at S6.
const EVENT_ROWS := 14
## The one file the lobby writes: the six-field config line of the last match it started,
## with no token in it.
const REMEMBERED := "user://last_match.txt"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink
@onready var editor: CanvasLayer = $Editor

var _status: Label
var _events: Label
var _rows: Array[String] = []
var _follow: Button
var _failed := false
var _started := false
var _resuming := false
## The six-field line written to the host, remembered once it announces.
var _line := ""
var _chooser: HBoxContainer
## Whether the lobby forgot the match because it ended; the status line then says so.
var _forgotten := false


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	_build_ui()
	vista.my_seat = HostLink.my_seat()
	vista.configure()
	link.bridge = vista.bridge
	link.failed.connect(_on_failed)
	link.received.connect(_on_received)
	link.announced.connect(_on_announced)
	editor.setup(vista)
	_status.text = Strings.text("lobby_choose")


## The line the lobby remembered from the last match it started, or "" when none is.
static func remembered_line() -> String:
	if not FileAccess.file_exists(REMEMBERED):
		return ""
	return FileAccess.get_file_as_string(REMEMBERED).strip_edges()


## Starts a new match under a fresh id.
func new_match() -> void:
	_start(HostLink.config_line(HostLink.match_id("local"), HostLink.LADDER), false)


## Goes on with the remembered match: the same six fields, plus `resume`.
func resume_match() -> void:
	var line := remembered_line()
	if line == "":
		return
	_start(line + "\n", true)


func _start(line: String, resuming: bool) -> void:
	if _started:
		return
	_started = true
	_resuming = resuming
	_line = line.strip_edges()
	_chooser.visible = false
	_status.text = Strings.text("lobby_starting")
	var written := HostLink.resume_line(line) if resuming else line
	if link.start(HostLink.find_gamectl(), HostLink.find_root(), written):
		editor.begin(HostLink.my_seat())


func _remember() -> void:
	var file := FileAccess.open(REMEMBERED, FileAccess.WRITE)
	if file != null:
		file.store_string(_line + "\n")
		file.close()


func _forget() -> void:
	if FileAccess.file_exists(REMEMBERED):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(REMEMBERED))
	_forgotten = true


func _process(_delta: float) -> void:
	link.pump()
	if _failed or not _started:
		return
	var state: Dictionary = vista.bridge.watch_state()
	if state.is_empty():
		return
	var parts: PackedStringArray = [Strings.text("lobby_status", {"round": state.get("round", 0), "phase": String(state.get("phase", "")).to_upper()})]
	var timer := String(state.get("timer", ""))
	if timer != "":
		parts.append(timer)
	parts.append(Strings.text("lobby_speed", {"speed": state.get("speed", 1)}))
	if state.get("skipping", false):
		parts.append(Strings.text("lobby_skipping"))
	if state.get("all_ready", false):
		parts.append(Strings.text("lobby_all_ready"))
	if _forgotten:
		parts.append(Strings.text("lobby_forgotten"))
	_status.text = "  ".join(parts)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST or what == NOTIFICATION_PREDELETE:
		if link != null:
			link.quit()


func _on_announced(_match_id: String) -> void:
	_status.text = Strings.text("lobby_connected")
	_remember()


func _on_failed(reason: String) -> void:
	_failed = true
	# A refusal is shown as the host wrote it (host_link.gd hands over its standard error).
	var key := "lobby_resumed_failed" if _resuming else "lobby_failed"
	_status.text = Strings.text(key, {"reason": reason})


func _on_received(answer: Dictionary) -> void:
	if answer.has("view"):
		var view: Dictionary = answer["view"]
		var first: bool = vista.marker_count() == 0
		vista.take_view(view)
		if first and view.get("complete", false):
			vista.frame_whole_map()
	if answer.get("phase_changed", false) and answer.get("phase", "") == "push":
		_rows.clear()
	if answer.get("phase_changed", false) and answer.get("phase", "") == "ended":
		# An ended match keeps its save and its id stays taken (item 113 (11)): the lobby
		# forgets it, so Resume does not replay its last Push.
		_forget()
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
	_chooser = HBoxContainer.new()
	column.add_child(_chooser)
	_button(_chooser, Strings.text("lobby_new_match"), new_match)
	if remembered_line() != "":
		_button(_chooser, Strings.text("lobby_resume"), resume_match)
	var buttons := HBoxContainer.new()
	column.add_child(buttons)
	# The speed set is the pacer's (crates/client-gdext/src/pacer.rs SPEEDS, a PLACEHOLDER);
	# the lobby only draws a button for each, so the set has one home.
	for speed in vista.bridge.watch_speeds():
		_button(buttons, Strings.text("lobby_speed_button", {"speed": speed}), func() -> void: vista.bridge.watch_command("speed", speed))
	_button(buttons, Strings.text("lobby_skip"), func() -> void: vista.bridge.watch_command("skip", 0))
	_button(buttons, Strings.text("lobby_ready"), func() -> void: vista.bridge.watch_command("ready", 0))
	_button(buttons, Strings.text("lobby_continue"), func() -> void: vista.bridge.watch_command("end_recap", 0))
	_follow = _button(buttons, Strings.text("lobby_follow"), _toggle_follow)
	_follow.toggle_mode = true
	_button(buttons, Strings.text("lobby_whole_map"), func() -> void: vista.frame_whole_map())
	_events = Label.new()
	column.add_child(_events)


func _button(parent: Node, text: String, pressed: Callable) -> Button:
	var button := Button.new()
	button.text = text
	button.accessibility_name = text
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(pressed)
	parent.add_child(button)
	return button


func _toggle_follow() -> void:
	if _follow.button_pressed:
		vista.rig.follow(vista.my_commander())
	else:
		vista.rig.follow(null)
