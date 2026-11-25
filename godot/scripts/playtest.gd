# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# PLAYTEST MODE (S3's `prob`, part 2; the plan's E10, decisions 1 and 11; AGENTS.md section 7,
# "No network telemetry"): the lobby's check button, the line that says it is on and what it
# records, the end-of-match survey, and Export playtest bundle.
#
# OPT-IN AND LOCAL. The check button is off at every launch and is not remembered: nothing is
# collected unless the tester turns it on in that session. Off again, the bridge drops everything
# it kept. Everything it keeps is in memory, in the bridge's recorder (crates/client-gdext/src/
# playtest.rs); nothing is written until the tester presses Export playtest bundle and picks a
# file, and then the bridge writes one zip there (`playtest_export`) and nowhere else. This script
# writes no file, reads no file, opens no connection and reads no clock: it draws, forwards, and
# asks the bridge.
#
# WHAT IT FORWARDS: the window's focus (Godot's application focus notifications), each camera
# move the camera rig says (`camera_rig.gd`'s `moved`), and the survey's answers. Each is one
# bridge call; the sums of time are the pacer's, in Rust, and this script does no arithmetic.
#
# THE SURVEY AND EXPORT are offered whenever Playtest mode is on and no match is running: once a
# match has ended, and whenever the lobby's chooser is shown (after a host gave up or stopped, or
# before any match), since the session a playtest most wants is the one that broke. What a
# stopped match left open is in the bundle as `match_left`. The survey asks: "Play another round?"
# (yes / no), "Did you understand why your playbook did what it did?" (yes / partly / no), and
# "What did you most want to change during a Push?" (free text). An unanswered question is sent
# as unanswered, never as a default answer. The free text goes into the bundle exactly as written,
# and the screen says so beside the box: the client cannot know what is private in it.
#
# PLACEHOLDER: Playtest mode's layout, the survey's controls and the export's words — OWNER, S6.

extends Node

const Strings := preload("res://scripts/strings.gd")
const Values := preload("res://scripts/values.gd")

## The name the export dialog offers: no user name, no date, no match id.
const BUNDLE_NAME := "pharmakos-playtest.zip"
## The survey's choices, by the bridge's names, in the order they are offered; the first item of
## each control is "no answer".
const ANOTHER := ["unanswered", "yes", "no"]
const UNDERSTOOD := ["unanswered", "yes", "partly", "no"]

## The PharmakosBridge.
var bridge: Node = null
var _toggle: CheckButton
var _note: Label
var _survey: VBoxContainer
var _another: OptionButton
var _understood: OptionButton
var _wanted: TextEdit
## How many characters the answer holds, against the most the bridge takes.
var _count: Label
## The most characters the answer may hold, as the bridge states it (`playtest_state`'s
## `survey_limit`); -1 before the bridge has said.
var _limit := -1
## What the export last said; it stays until the answer's text changes.
var _said: Label
var _export: Button
var _dialog: FileDialog
## The lobby's chooser: shown exactly while no match is running.
var _chooser: Container = null


## Builds the controls: the check button in `chooser` (the lobby's New match row), the line that
## says it is on and the survey in `column`. Camera moves are counted from `rig`'s `moved`.
func setup(vista_bridge: Node, rig: Node, chooser: Container, column: Container) -> void:
	bridge = vista_bridge
	_chooser = chooser
	_toggle = CheckButton.new()
	_toggle.text = Strings.text("playtest_toggle")
	_toggle.accessibility_name = _toggle.text
	_toggle.focus_mode = Control.FOCUS_NONE
	_toggle.button_pressed = false
	_toggle.toggled.connect(switch)
	chooser.add_child(_toggle)
	_note = Label.new()
	_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_note.custom_minimum_size = Vector2(560, 0)
	_note.visible = false
	column.add_child(_note)
	_build_survey(column)
	if rig != null and rig.has_signal("moved"):
		rig.moved.connect(camera_moved)
	_dialog = FileDialog.new()
	_dialog.file_mode = FileDialog.FILE_MODE_SAVE_FILE
	_dialog.access = FileDialog.ACCESS_FILESYSTEM
	_dialog.filters = PackedStringArray(["*.zip"])
	_dialog.use_native_dialog = true
	_dialog.file_selected.connect(export_to)
	add_child(_dialog)


func _build_survey(column: Container) -> void:
	_survey = VBoxContainer.new()
	_survey.visible = false
	column.add_child(_survey)
	_survey.add_child(_label(Strings.text("playtest_survey_heading")))
	_survey.add_child(_label(Strings.text("playtest_q_another")))
	_another = _choices(ANOTHER, Strings.text("playtest_q_another"))
	_survey.add_child(_label(Strings.text("playtest_q_understood")))
	_understood = _choices(UNDERSTOOD, Strings.text("playtest_q_understood"))
	_survey.add_child(_label(Strings.text("playtest_q_wanted")))
	_wanted = TextEdit.new()
	_wanted.custom_minimum_size = Vector2(560, 80)
	_wanted.accessibility_name = Strings.text("playtest_q_wanted")
	_wanted.text_changed.connect(_wanted_changed)
	_survey.add_child(_wanted)
	_count = _label("")
	_survey.add_child(_count)
	_survey.add_child(_label(Strings.text("playtest_wanted_note")))
	_export = Button.new()
	_export.text = Strings.text("playtest_export")
	_export.accessibility_name = _export.text
	_export.focus_mode = Control.FOCUS_NONE
	_export.pressed.connect(open_export)
	_survey.add_child(_export)
	_said = _label("")
	_survey.add_child(_said)


func _label(text: String) -> Label:
	var label := Label.new()
	label.text = text
	label.accessibility_name = text
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.custom_minimum_size = Vector2(560, 0)
	return label


## One question's choices, named from the string table's `playtest_answer_<name>` rows.
func _choices(names: Array, question: String) -> OptionButton:
	var choices := OptionButton.new()
	choices.accessibility_name = question
	for name in names:
		choices.add_item(Strings.text("playtest_answer_" + String(name)))
	choices.select(0)
	_survey.add_child(choices)
	return choices


## The check button: Playtest mode on or off, with the window's focus as it is now. The button
## shows what the bridge says, not what was pressed.
func switch(on: bool) -> void:
	var now_on: bool = bridge.playtest_set(on, get_window().has_focus())
	_toggle.set_pressed_no_signal(now_on)
	if not now_on:
		_said.text = ""
		_wanted.text = ""
		_another.select(0)
		_understood.select(0)


func _notification(what: int) -> void:
	if bridge == null or _toggle == null or not _toggle.button_pressed:
		return
	if what == NOTIFICATION_APPLICATION_FOCUS_IN:
		bridge.playtest_focus(true)
	elif what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		bridge.playtest_focus(false)


## A camera move the rig said, counted by kind while Playtest mode is on.
func camera_moved(kind: String) -> void:
	if _toggle != null and _toggle.button_pressed:
		bridge.playtest_camera(kind)


## Off, Playtest mode costs nothing: no bridge call, its line and survey hidden. On, it draws the
## bridge's `playtest_state` each frame.
func _process(_delta: float) -> void:
	if bridge == null or _toggle == null or not _toggle.button_pressed:
		_note.visible = false
		_survey.visible = false
		return
	show_state(bridge.playtest_state())


## Draws Playtest mode's line and the survey from the bridge's `playtest_state`: the line while it
## is on, the survey and Export while no match is running (the match ended, or the chooser is
## shown). A branch on the bridge's phase word and the chooser's visibility, for display.
func show_state(state: Dictionary) -> void:
	var on: bool = state.get("on", false)
	_note.visible = on
	if on:
		_note.text = note_text(state)
		_note.accessibility_name = _note.text
	var stopped := _chooser != null and _chooser.is_visible_in_tree()
	_survey.visible = on and (String(state.get("phase", "")) == "ended" or stopped)
	var limit: Variant = Values.int_of(state.get("survey_limit", -1))
	if limit is int and limit != _limit:
		_limit = limit
		_count_text()


## The line shown while Playtest mode is on: what it records, and the counts so far, as sent.
static func note_text(state: Dictionary) -> String:
	var lines: PackedStringArray = [Strings.text("playtest_on")]
	lines.append(Strings.text("playtest_counts", {
		"pushes": Values.said(Strings.text("playtest_counts"), state.get("pushes", 0)),
		"skipped": Values.said(Strings.text("playtest_counts"), state.get("skipped_pushes", 0)),
		"lulls": Values.said(Strings.text("playtest_counts"), state.get("lulls", 0)),
	}))
	if state.get("dropped", false):
		lines.append(Strings.text("playtest_dropped"))
	return "\n".join(lines)


## The answer's text changed: the export's last words go, and the count is drawn again. The text
## goes nowhere until Export.
func _wanted_changed() -> void:
	_said.text = ""
	_said.accessibility_name = ""
	_count_text()


## The answer's length against the most the bridge takes, as the bridge states the cap.
func _count_text() -> void:
	_count.text = Strings.text("playtest_wanted_count", {"count": _wanted.text.length(), "limit": _limit})
	_count.accessibility_name = _count.text


## Export playtest bundle: the save dialog, offering the bundle's name in no folder of ours.
func open_export() -> void:
	_dialog.current_file = BUNDLE_NAME
	_dialog.popup_centered_ratio(0.6)


## The bundle written to `path`, the file the tester picked, by the bridge, with the survey's
## answers as the screen shows them now; what happened is said under the survey, the path shown to
## the tester and sent nowhere. A refused export writes no file, and its words stay until the
## answer's text changes.
func export_to(path: String) -> Dictionary:
	var written: Dictionary = bridge.playtest_export(ProjectSettings.globalize_path(path), ANOTHER[_another.selected], UNDERSTOOD[_understood.selected], _wanted.text)
	_said.text = export_words(written, path)
	_said.accessibility_name = _said.text
	return written


## The words for the bridge's answer to an export of `path`.
static func export_words(written: Dictionary, path: String) -> String:
	if written.has("refusal"):
		return Strings.text("playtest_export_" + String(written["refusal"]), {"limit": written.get("limit", "")})
	var why := Strings.write_words(written)
	if not written.get("written", false):
		return Strings.text("playtest_export_not_done", {"why": why})
	if why != "":
		return Strings.text("playtest_exported_note", {"path": path, "why": why})
	return Strings.text("playtest_exported", {"path": path, "bytes": Values.said(Strings.text("playtest_export"), written.get("bytes", 0))})
