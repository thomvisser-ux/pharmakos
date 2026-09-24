# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The diagnostic rows, drawn with NO host running: T19's render-only scene
# (skeleton-plan-w6-notes.md section A4).
#
# `godot/fixtures/rows_report.json` is a `gp.api.v1.VerifyReport` assembled from four of
# the verifier's own committed reports (tests/golden/verifier: E0003 with a suggestion that
# needs review, E0108 with a machine-applicable Fix, E0008 a warning with a Fix, E0403 with
# none). It goes through the bridge's own row decode (`editor_rows_of_report`) and the
# editor's own row drawing (`scripts/rows.gd`), so the picture is the editor's picture of
# those diagnostics: an icon and the severity in words, the plain sentence, the code and the
# pointer, and a Fix button for each machine-applicable patch only.
#
# RENDER-ONLY: the shot is taken and looked at, and NOT compared. `cargo xtask screenshot`
# compares the vista alone, and decisions-log item 110 (4) books a render-only mode to T20,
# which commits this shot as a golden and compares it.
#
# `--shot=<png>` names where the PNG goes; it is taken windowed, never `--headless`, for
# the same reason as the vista's shot (scripts/vista_shot.gd). Without `--shot=` the scene
# draws the rows, checks that each row's accessible name is its sentence, and quits: a
# smoke test that runs headless.
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/rows_shot.tscn --shot=<png>
#
# Exit codes: 0 when the rows were drawn (and shot, when asked) with no caught panic, 1
# when anything failed.

extends Control

const Rows := preload("res://scripts/rows.gd")
const FIXTURE := "res://fixtures/rows_report.json"
## Frames drawn before the frame that is read back, so the layout has settled.
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var rows: VBoxContainer = $Panel/Rows


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var text := FileAccess.get_file_as_string(FIXTURE)
	var drawn: Array = bridge.editor_rows_of_report(text)
	if drawn.is_empty():
		_fail("the fixture did not decode; the reason is in the log")
		return
	Rows.fill(rows, drawn, func(_row: int, _fix: int) -> void: pass, true)
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	for control in Rows.row_controls(rows):
		if control.accessibility_name != String(control.get_meta("sentence")):
			_fail("a row's accessible name is not its sentence")
			return
	if bridge.caught_panics() != 0:
		_fail("%d panic(s) were caught at the bridge boundary" % bridge.caught_panics())
		return
	if shot != "":
		await RenderingServer.frame_post_draw
		var image := get_viewport().get_texture().get_image()
		var error := image.save_png(shot)
		if error != OK:
			_fail("the PNG could not be written to %s (error %d)" % [shot, error])
			return
		print("[rows-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[rows-shot] %d rows drawn; OK" % drawn.size())
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[rows-shot] FAILED: %s" % message)
	get_tree().quit(1)
