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
# decides that from the host clock's answer and calls `end_lull` itself. Round 1's Lull is
# timed by the rules table's `first_lull_ms` and every later one by `lull_ms`, and the status
# line shows no countdown while the admin footer says the phase has none (`_status.untimed`;
# the bridge's `timer` is then empty; decisions-log item 135). Ready is this seat's
# `set_ready`, and its label and the line under it, shown in a Lull, say what it does: with
# nothing sealed the gateway files the safe playbook (decisions-log item 143 (4)), and with a
# seal the gateway accepted this round Ready keeps it (`ready_line`, S3's `fixp`; the gateway's own
# word on the seal, `get_status`'s, since S3's `fixq-c`). Continue
# leaves the recap.
#
# THE RECAP (S1's plan, task `ui`): in a recap and once the match has ended, the column
# shows `get_recap`'s prose as the gateway wrote it - the settlement's line, the shortfall's
# and why a step found nothing ("3 matched, none reachable") are its sentences; a refused read
# shows the gateway's refusal as it came (a lost segment_ended, item 135 (1)). Whether it is
# shown is a branch on the bridge's phase string, for display, as the speed label's is. Every button is a request to the bridge;
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
#   * A REFUSED RESUME (S3's `prob`; the plan's E6): whatever the cause - a line remembered
#     before the preset existed has six fields, and the gateway refuses it naming the field -
#     the lobby shows the host's refusal as it came and offers the chooser again with New
#     match and no Resume. It reads nothing but "a Resume was refused": it parses no word of
#     the refusal and counts no field of the line. The remembered line stays until New match
#     announces and overwrites it, so a refusal this client cannot type loses no match.
#   * A MATCH THAT FAILS (prob's fix pass 1): any failure the link reports - before the host
#     announced or after, of a new match or a resumed one - is shown as it came, and the lobby
#     lets the host go and offers the chooser again, with New match, and Resume when a line is
#     remembered and the failure was not that line's refused Resume. So the words on screen
#     always name a button that is there, and the chooser and the watch strip are never shown
#     together. `_resuming` means "a Resume was asked and no announce has come": it is cleared
#     on the announce.
# The New match button names the preset it will ask for (HostLink.PRESET, through the string
# table's `preset_<name>` rows), and once a match is announced the status line names the
# preset the gateway says it hosts (`get_status`'s `preset`, read by the bridge's rig) through
# the same rows; a hosted preset that is not the one New match asked for is said as a
# refusal. Nothing here states what a preset does: that is the gateway's and the rules'.
# Once the host has announced, the lobby remembers the six-field line it wrote, in ONE
# `user://` file that holds no token (the tokens stay in host_link.gd's variables).
# Remembering the whole line rather than the id alone means a resume repeats exactly what
# the save was made with, even if a constant in host_link.gd changes between builds. When
# the match ends, the lobby forgets it: an ended match keeps its save and its id stays
# taken (item 113 (11)), so a finished match's last Push is not replayed from here.
#
# BEFORE A MATCH (F6, decisions-log item 126 (3)): the chooser alone. The watch strip (the
# speed buttons, Skip, Ready, Continue, Follow, Whole map) and the editor's panel are hidden
# until the host has announced a match, and the status line names the watch speed only in
# a Push, where speed means something.
#
# With `-- --log-input` on the command line, `_input` prints every mouse button the lobby
# sees, beside the editor's own logging of Submit, Fix and Save (scripts/editor.gd): the
# owner's way to reproduce F1 by hand (godot/README.md).
#
# PLACEHOLDER: the remembered match in `user://` — OWNER, S6, with the save browser. It is
# a per-machine convenience, not state: the save itself is the gateway's, in the private
# match cache.
# PLACEHOLDER: forgetting an ended match and the New/Resume layout — OWNER, S6. Forgetting
# follows item 113 (11).
#
# PLACEHOLDER: the lobby's whole layout — OWNER, S6, with the real lobby. A strip of
# buttons and a text column over the vista, the recap's line among them, are the skeleton's.

extends Node

const HostLink := preload("res://scripts/host_link.gd")
const Strings := preload("res://scripts/strings.gd")
const Values := preload("res://scripts/values.gd")
## The credits overlay the chooser's Credits button adds (decisions-log item 117 (11)).
const AboutScene := preload("res://scenes/credits.tscn")

## How many event rows the list keeps on screen. PLACEHOLDER, UI, OWNER at S6.
const EVENT_ROWS := 14
## The one file the lobby writes: the six-field config line of the last match it started,
## with no token in it. It is written whole or not at all (`safe_write.gd`, S3's `fixq-c`,
## fix pass 3), through `<file>.tmp` and `<file>.bak` beside it; its format is unchanged.
const REMEMBERED := "user://last_match.txt"

@onready var vista: Node3D = $Vista
@onready var link: Node = $HostLink
@onready var editor: CanvasLayer = $Editor

var _status: Label
var _recap: Label
## The line under the watch strip that says what Ready does, in a Lull.
var _ready_help: Label
var _events: Label
var _rows: Array[String] = []
var _follow: Button
var _started := false
var _resuming := false
## The six-field line written to the host, remembered once it announces.
var _line := ""
var _chooser: HBoxContainer
## The chooser's Resume button, shown only while a line is remembered.
var _resume_button: Button = null
## Whether the host announced a match: a failure before it is a refused start.
var _announced := false
## The watch strip: shown once a match has started.
var _strip: HBoxContainer
## Whether the lobby forgot the match because it ended; the status line then says so.
var _forgotten := false
## The file the remembered line is kept in: REMEMBERED, unless a check points the lobby at a
## file of its own before it enters the tree.
var remembered := REMEMBERED
## Why the remembered file, which exists, could not be read, in the string table's words; ""
## when it could (or there is none). Said on the chooser's line.
var _remembered_unreadable := ""
## Why this match's line could not be remembered, or why the ended match could not be
## forgotten, in the string table's words; "" when it was. The status line says it.
var _remember_failed := ""
var _forget_failed := ""


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
	editor.visible = false
	_status.text = Strings.text("lobby_choose")
	if _remembered_unreadable != "":
		_status.text += "  " + _remembered_unreadable


## The line the lobby remembered from the last match it started, or "" when none is: an absent
## file, an empty one (an ended match forgotten by emptying it) and a folder are all none. The
## line itself is read by the host, which refuses a short or malformed one in its own words.
func remembered_line() -> String:
	var path := ProjectSettings.globalize_path(remembered)
	_remembered_unreadable = ""
	if not FileAccess.file_exists(path):
		return ""
	# A file that exists and cannot be read is said, not read as no match (fix pass 4).
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		_remembered_unreadable = Strings.text("lobby_remembered_unreadable", {"path": path, "error": FileAccess.get_open_error()})
		return ""
	var text := file.get_as_text()
	file.close()
	return text.strip_edges()


## Starts a new match under a fresh id, under HostLink.PRESET, which the bridge then holds the
## gateway's `get_status` answer to.
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
		if not resuming:
			vista.bridge.watch_expect_preset(HostLink.PRESET)


## Remembers this match's line, whole or not at all, by an atomic replace through the bridge
## (`crates/client-gdext/src/write.rs`); a line that could not be written is said on the status
## line, which then claims no remembered match. The bytes are what `main`'s writer wrote: the
## six-field line and a newline, UTF-8.
func _remember() -> void:
	var written: Dictionary = vista.bridge.file_write(ProjectSettings.globalize_path(remembered), (_line + "\n").to_utf8_buffer())
	_remember_failed = ""
	if not written.get("written", false):
		_remember_failed = Strings.text("lobby_not_remembered", {"why": Strings.write_words(written)})
	elif Strings.write_words(written) != "":
		_remember_failed = Strings.text("lobby_remembered_note", {"why": Strings.write_words(written)})


## Forgets the ended match: its file removed through the bridge. A removal that fails (a folder,
## a read-only file, one another program holds) is said on the status line with its cause, and
## says to start a new match. (A Resume of an ended match is not refused by the host today:
## reported in PR #229 for the gateway's lane, which lane `opsave` carries.)
func _forget() -> void:
	var removed: Dictionary = vista.bridge.file_remove(ProjectSettings.globalize_path(remembered))
	_forget_failed = ""
	if removed.get("written", false):
		_forgotten = true
		return
	_forgotten = false
	_forget_failed = Strings.text("lobby_not_forgotten", {"why": Strings.write_words(removed)})


func _process(_delta: float) -> void:
	link.pump()
	if not _started:
		return
	var state: Dictionary = vista.bridge.watch_state()
	if state.is_empty():
		return
	_status.text = status_line(state)
	_ready_help.visible = String(state.get("phase", "")) == "lull"
	if _ready_help.visible:
		_ready_help.text = ready_line(state)
		_ready_help.accessibility_name = _ready_help.text
	_draw_recap(String(state.get("phase", "")))


## The status line for the watch state `state`: the round and phase, the timer, the speed, and
## what the lobby and the bridge have to say (a forgotten or unremembered match, a close on a
## message, the preset, a refused zero), joined.
func status_line(state: Dictionary) -> String:
	var parts: PackedStringArray = [Strings.text("lobby_status", {"round": state.get("round", 0), "phase": String(state.get("phase", "")).to_upper()})]
	var timer := String(state.get("timer", ""))
	if timer != "":
		parts.append(timer)
	if String(state.get("phase", "")) == "push":
		parts.append(Strings.text("lobby_speed", {"speed": state.get("speed", 1)}))
	if state.get("skipping", false):
		parts.append(Strings.text("lobby_skipping"))
	if state.get("all_ready", false):
		parts.append(Strings.text("lobby_all_ready"))
	if _forgotten:
		parts.append(Strings.text("lobby_forgotten"))
	var closed_on := closed_on_line(state)
	if closed_on != "":
		parts.append(closed_on)
	if _remember_failed != "":
		parts.append(_remember_failed)
	if _forget_failed != "":
		parts.append(_forget_failed)
	if _remembered_unreadable != "":
		parts.append(_remembered_unreadable)
	var preset := preset_line(state)
	if preset != "":
		parts.append(preset)
	# The gateway kept refusing the Lull's closing countdown at zero: the rig gave it up and
	# ended the Lull without it, and the line says so in the gateway's words.
	var zero_refused := String(state.get("zero_refusal", ""))
	if zero_refused != "":
		parts.append(Strings.text("lobby_zero_refused", {"detail": zero_refused}))
	return "  ".join(parts)


## The words for the last close on a message (fix pass 3): which connection, the close code,
## the call it carried (or none in flight), and whether that connection is open again, all as
## the bridge's watch state says them (`closed_on`); "" when there was none. A branch on the
## bridge's words, for display.
static func closed_on_line(state: Dictionary) -> String:
	if not state.has("closed_on"):
		return ""
	var closed: Dictionary = state["closed_on"]
	var call := "none"
	if closed.has("call"):
		call = String(closed["call"])
	return Strings.text("lobby_closed_on_message", {
		"connection": Strings.text("connection_" + String(closed["connection"])),
		"code": closed["code"],
		"call": Strings.text("call_word_" + call),
		"state": Strings.text("link_open" if closed["open"] else "link_reopening"),
	})


## The preset's name as the string table spells it: the `preset_<name>` row for the wire's
## name (`probation`, `custom`). It names; it never says what the preset does.
static func preset_name(wire: String) -> String:
	return Strings.text("preset_" + wire)


## The New match button's words: the preset New match will write (HostLink.PRESET), named.
static func new_match_text() -> String:
	return Strings.text("lobby_new_match", {"preset": preset_name(HostLink.PRESET)})


## The status line's words for the hosted match's preset, from the bridge's reading of
## `get_status` (`preset`, `preset_check`, `preset_asked`, `status_failed`, `status_detail`):
## the preset named, a refusal when it is not the one New match asked for, "" before the first
## answer. A newest read that did not come is said beside the last one's name. A branch on the
## bridge's word, for display.
static func preset_line(state: Dictionary) -> String:
	var check := String(state.get("preset_check", ""))
	var served := String(state.get("preset", ""))
	var said := ""
	match check:
		"agrees", "unasked":
			said = Strings.text("lobby_preset", {"preset": preset_name(served)})
		"disagrees":
			said = Strings.text("lobby_preset_disagrees", {"asked": preset_name(String(state.get("preset_asked", ""))), "served": preset_name(served)})
		"no_match":
			said = Strings.text("lobby_preset_none")
	var failed := String(state.get("status_failed", ""))
	if failed != "":
		var why := Strings.text("lobby_status_" + failed, {"detail": state.get("status_detail", "")})
		said = why if said == "" else "%s %s" % [said, why]
	return said


## What Ready will do now, in a Lull (S3's `fixp`, P6b finding 4), from the gateway's own word on
## this seat's seal: the watch state's `sealed_this_round`, the rig's reading of the seat's
## `get_status` for this round and phase, and the one source of it (S3's `fixq-c`, item 13: the
## editor keeps no record of its own, so a resumed client says what the gateway says). Sealed,
## Ready keeps it; not sealed, the gateway files the safe playbook; not told yet, the line says the
## client is asking. A branch on the bridge's word, for display.
static func ready_line(watch_state: Dictionary) -> String:
	# A Ready that did not go through, whatever stopped it, is said on this line (fix pass 4).
	if watch_state.has("ready_failed"):
		return Strings.text("ready_failed_" + String(watch_state["ready_failed"]), {"detail": watch_state.get("ready_failed_detail", "")})
	if not watch_state.has("sealed_this_round"):
		# Asked a few times and refused each time: the gateway's refusal, never "Asking..." while
		# nobody is asking (fix pass 2).
		if watch_state.get("seal_not_told", false):
			return Strings.text("lobby_ready_not_told", {"detail": watch_state.get("status_detail", "")})
		return Strings.text("lobby_ready_asking")
	return Strings.text("lobby_ready_sealed" if watch_state.get("sealed_this_round", false) else "lobby_ready_help")


## The recap's prose, as the gateway wrote it, in a recap and after the match has ended;
## or, when the gateway refused the read, its refusal as it came.
func _draw_recap(phase: String) -> void:
	var recap: Dictionary = vista.bridge.watch_recap()
	var refusal := String(recap.get("refusal", ""))
	var answers: Variant = Values.int_of(recap.get("answers", 0))
	if not (answers is int):
		# A count past `int`: the recap's line says it cannot be read, in a recap and after the end.
		_recap.visible = phase == "recap" or phase == "ended"
		_recap.text = Values.unreadable_text(Strings.text("value_recap_answers"), answers)
		return
	var answered: bool = answers > 0 or refusal != ""
	var shown: bool = (phase == "recap" or phase == "ended") and answered
	_recap.visible = shown
	if shown:
		if refusal != "":
			_recap.text = Strings.text("lobby_recap_refused", {"refusal": refusal})
		else:
			_recap.text = recap_text_of(recap)
		_recap.accessibility_name = _recap.text


## The recap's words: the gateway's prose under its heading (its licence sentence among them,
## drawn whole), then every seat's tally when the answer carries them, which it does once the
## match is decided (item 145 (7)), then the Ledger's memos to this seat in the order they were
## issued, each as the gateway wrote it, under a heading only when there are any (a custom
## match is sent none). Each number is put into its frame as it came.
static func recap_text_of(recap: Dictionary) -> String:
	var lines: PackedStringArray = [Strings.text("lobby_recap", {"round": recap.get("round", 0), "prose": recap.get("prose", "")})]
	var seats: Array = recap.get("seat_tallies", [])
	if not seats.is_empty():
		lines.append(Strings.text("lobby_recap_tallies"))
	for seat in seats:
		lines.append(Strings.text("lobby_recap_tally", {"seat": seat.get("seat", ""), "kills": seat.get("kills", 0), "shared_kills": seat.get("shared_kills", 0), "value": seat.get("value_destroyed_dollars", 0), "losses": seat.get("losses", 0)}))
	var memos: Array = recap.get("memos", [])
	if not memos.is_empty():
		lines.append(Strings.text("lobby_recap_memos"))
	for memo in memos:
		lines.append(Strings.text("lobby_recap_memo", {"text": memo.get("text", "")}))
	return "\n".join(lines)


## One event-list row as the lobby draws it: an ordinary event as the bridge spelt it (its
## time, kind and words), and one of the Ledger's memos under the Ledger's speaker word in
## place of the kind (the bridge's `ledger`, read from the event's typed payload).
static func event_text_of(row: Dictionary) -> String:
	if row.get("ledger", false):
		return Strings.text("event_ledger", {"clock": row.get("clock", ""), "text": row.get("text", "")})
	return String(row.get("line", ""))


## The recap as drawn, for a check to read.
func recap_text() -> String:
	return _recap.text if _recap.visible else ""


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST or what == NOTIFICATION_PREDELETE:
		if link != null:
			link.quit()


func _input(event: InputEvent) -> void:
	if editor.log_input and event is InputEventMouseButton:
		print("[lobby] _input: button %d %s at %s, alt_pressed %s, KEY_ALT held %s; %s" % [event.button_index, "down" if event.pressed else "up", event.position, event.alt_pressed, Input.is_key_pressed(KEY_ALT), editor.dormant_log(event.position)])


func _on_announced(_match_id: String) -> void:
	_announced = true
	# The Resume that was asked has been answered: from here a failure is the match's, not a
	# refused Resume's.
	_resuming = false
	_status.text = Strings.text("lobby_connected")
	_strip.visible = true
	editor.visible = true
	_remember()


func _on_failed(reason: String) -> void:
	# A refusal is shown as the host wrote it (host_link.gd hands over its standard error).
	var key := "lobby_resumed_failed" if _resuming else "lobby_failed"
	_status.text = Strings.text(key, {"reason": reason})
	# A refused Resume, whatever its cause, offers New match and no Resume; any other failure
	# offers Resume too when a line is remembered. The words are shown and never read; the
	# remembered line stays.
	_offer_chooser(not _resuming)


## After a failure: the link lets go of the host (host_link.gd's `reset`, deferred, so the link
## is not reset from inside its own signal), the match's controls are hidden, and the chooser
## is shown again with New match, and with Resume when `resume_too` and a line is remembered.
func _offer_chooser(resume_too: bool) -> void:
	link.reset.call_deferred()
	_started = false
	_resuming = false
	_announced = false
	_line = ""
	_strip.visible = false
	_ready_help.visible = false
	_recap.visible = false
	editor.visible = false
	_resume_button.visible = resume_too and remembered_line() != ""
	if _remembered_unreadable != "":
		_status.text += "  " + _remembered_unreadable
	_chooser.visible = true


func _on_received(answer: Dictionary) -> void:
	if answer.has("view"):
		var view: Dictionary = answer["view"]
		var first: bool = vista.marker_count() == 0
		vista.take_view(view)
		if first and view.get("complete", false):
			if not vista.open_on_own_commander(editor.PANEL_WIDTH):
				vista.frame_whole_map()
	if answer.get("phase_changed", false) and answer.get("phase", "") == "push":
		_rows.clear()
	if answer.get("phase_changed", false) and answer.get("phase", "") == "ended":
		# An ended match keeps its save and its id stays taken (item 113 (11)): the lobby
		# forgets it, so Resume does not replay its last Push.
		_forget()
	for row in answer.get("events", []):
		_rows.append(event_text_of(row))
	while _rows.size() > EVENT_ROWS:
		_rows.pop_front()
	_events.text = "\n".join(_rows)
	var error := String(answer.get("error", ""))
	if error != "":
		push_warning("[lobby] the gateway refused a call: %s" % error)


func _build_ui() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	# The lobby's layer is drawn over the editor's, and a container passes the mouse by
	# default, so a column widened by a long event row would sit over the editor's panel
	# and take the clicks meant for its buttons (F1). The containers here ignore the mouse;
	# their buttons still take it.
	var column := VBoxContainer.new()
	column.position = Vector2(12, 12)
	column.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(column)
	_status = Label.new()
	column.add_child(_status)
	_recap = Label.new()
	_recap.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_recap.custom_minimum_size = Vector2(560, 0)
	_recap.visible = false
	column.add_child(_recap)
	_chooser = HBoxContainer.new()
	_chooser.mouse_filter = Control.MOUSE_FILTER_IGNORE
	column.add_child(_chooser)
	_button(_chooser, new_match_text(), new_match)
	_resume_button = _button(_chooser, Strings.text("lobby_resume"), resume_match)
	_resume_button.visible = remembered_line() != ""
	_button(_chooser, Strings.text("lobby_about"), _show_about)
	var buttons := HBoxContainer.new()
	buttons.mouse_filter = Control.MOUSE_FILTER_IGNORE
	buttons.visible = false
	_strip = buttons
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
	_ready_help = Label.new()
	_ready_help.text = Strings.text("lobby_ready_help")
	_ready_help.accessibility_name = _ready_help.text
	_ready_help.visible = false
	column.add_child(_ready_help)
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


## The Credits button: the overlay covers the chooser until its Back, and is never a scene
## change (freeing the lobby ends its host). The button is in the chooser alone, which is
## hidden once a match has started, so no overlay opens while a host runs.
func _show_about() -> void:
	if _started:
		return
	var overlay: CanvasLayer = AboutScene.instantiate()
	overlay.closed.connect(func() -> void: _chooser.visible = true)
	_chooser.visible = false
	add_child(overlay)


func _toggle_follow() -> void:
	if _follow.button_pressed:
		vista.rig.follow(vista.my_commander())
	else:
		vista.rig.follow(null)
