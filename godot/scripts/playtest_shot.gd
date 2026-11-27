# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Playtest mode drawn with NO host running (S3's `prob`, part 2): the chooser's check button
# switched on, the line that says what it records and that nothing leaves the machine, and the
# end-of-match survey with Export playtest bundle, each drawn by `scripts/playtest.gd` itself over
# the bridge's own recorder.
#
# Without `--shot=` it is a check: the button switched the bridge's recorder on, the line and the
# survey are shown, an answer past its cap refuses the export in the string table's words until
# the answer changes, the check button switched off clears the recorder, and the scene quits 0.
# COMPARED: `cargo xtask screenshot` renders it and compares it with
# `tests/golden/vista/expected.playtest.png` (`xtask`'s SHOTS, from S3's `cert3`). It can also
# be run by hand (godot/README.md):
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/playtest_shot.tscn --shot=<png>
#
# Exit codes: 0 when everything was drawn and answered as it should be (and shot, when asked) with
# no caught panic, 1 when anything failed.

extends Control

const Playtest := preload("res://scripts/playtest.gd")
const Strings := preload("res://scripts/strings.gd")
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var column: VBoxContainer = $Column


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var chooser := HBoxContainer.new()
	column.add_child(chooser)
	var playtest: Node = Playtest.new()
	add_child(playtest)
	playtest.setup(bridge, null, chooser, column)
	# Hostless, the bridge's phase is "": the survey is drawn as the lobby draws it once a match
	# has ended.
	playtest.set_process(false)
	playtest.switch(true)
	var state: Dictionary = bridge.playtest_state()
	if not state.get("on", false):
		_fail("the check button did not switch Playtest mode on: %s" % state)
		return
	state["phase"] = "ended"
	playtest.show_state(state)
	if not playtest._note.visible or not playtest._survey.visible:
		_fail("the line or the survey is not shown with Playtest mode on at the match's end")
		return
	if not playtest._note.text.begins_with(Strings.text("playtest_on")):
		_fail("the line does not say what Playtest mode records: %s" % playtest._note.text)
		return
	# A refused export: the answer past the cap the bridge states, to a folder that is not there,
	# so nothing could be written even if the refusal failed. Its words stay until the text changes.
	var limit: int = int(state.get("survey_limit", 0))
	playtest._wanted.text = "x".repeat(limit + 1)
	playtest._wanted_changed()
	var refused: Dictionary = playtest.export_to(ProjectSettings.globalize_path("user://playtest_shot_no_such_folder/x.zip"))
	var words := Strings.text("playtest_export_survey_too_long", {"limit": limit})
	if refused.get("written", true) or String(refused.get("refusal", "")) != "survey_too_long" or playtest._said.text != words:
		_fail("an answer past its cap did not refuse the export in the string table's words: %s, `%s`" % [refused, playtest._said.text])
		return
	playtest._wanted.text = "The rule list: I wanted to see why my scout turned back."
	playtest._wanted_changed()
	if playtest._said.text != "":
		_fail("the refusal's words stayed after the answer changed")
		return
	if playtest._count.text != Strings.text("playtest_wanted_count", {"count": playtest._wanted.text.length(), "limit": limit}):
		_fail("the answer's count is not drawn against the bridge's cap: `%s`" % playtest._count.text)
		return
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var caught: Variant = bridge.caught_panics()
	if not (caught is int) or caught != 0:
		_fail("panic(s) were caught at the bridge boundary: %s" % str(caught))
		return
	if shot != "":
		await RenderingServer.frame_post_draw
		var image := get_viewport().get_texture().get_image()
		var error := image.save_png(shot)
		if error != OK:
			_fail("the PNG could not be written to %s (error %d)" % [shot, error])
			return
		print("[playtest-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	playtest.switch(false)
	if bridge.playtest_state().get("on", true) or bridge.playtest_state().has("pushes"):
		_fail("switched off, the recorder still holds something: %s" % bridge.playtest_state())
		return
	print("[playtest-shot] the check button on, its line and the survey drawn, a long answer refused, switched off and cleared; OK")
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[playtest-shot] FAILED: %s" % message)
	get_tree().quit(1)
