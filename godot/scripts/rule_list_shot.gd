# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The rule list, drawn with NO host running (S3's `ed3b`): the render-only scene for the rule
# list's screenshot, as `rows_shot.gd` is the rows'.
#
# `godot/fixtures/rule_list_render.json` is a byte-identical copy of the gateway's own render
# golden for the Expand & Mine demo (tests/golden/gateway/demo_expand_and_mine_render/
# expected.response.json; godot_project.rs keeps the two identical). It goes through the
# bridge's own decode and cut (`editor_rule_list_of_response`) and the editor's own rule-list
# drawing (`scripts/rule_list.gd`), so the picture is the editor's picture of that answer:
# each sentence with its chips drawn as links, the duration dim, and a conflict connector
# joining the route's second step and the rule, as a W0702 row would name them.
#
# NOT YET COMPARED: `cargo xtask screenshot` renders the shots `xtask`'s SHOTS lists, and
# adding this one is an `xtask` change this lane does not own; until a lane adds it, the scene
# is run by hand (godot/README.md), and without `--shot=` it is a smoke test that draws the
# list, checks each row's accessible name is its line, and quits.
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/rule_list_shot.tscn --shot=<png>
#
# Exit codes: 0 when the list was drawn (and shot, when asked) with no caught panic, 1 when
# anything failed.

extends Control

const RuleList := preload("res://scripts/rule_list.gd")
const FIXTURE := "res://fixtures/rule_list_render.json"
## Frames drawn before the frame that is read back, so the layout has settled.
const SETTLE_DRAWS := 4
## The connector the shot draws, by rule index: the route's second step and the first rule.
const SHOWN_CONFLICT := {"earlier": 1, "later": 2, "sentence": "This rule is true whenever the step above it runs."}

@onready var bridge: Node = $Bridge
@onready var rules: VBoxContainer = $Panel/Rules


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var list: Dictionary = bridge.editor_rule_list_of_response(FileAccess.get_file_as_string(FIXTURE))
	if list.is_empty():
		_fail("the fixture did not decode; the reason is in the log")
		return
	rules.draw.connect(func() -> void: RuleList.draw_connectors(rules))
	RuleList.fill(rules, list, 0, true, -1, [SHOWN_CONFLICT], {}, func(_chip: int) -> void: pass, func(_from: int, _to: int, _revision: int) -> void: pass)
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var lines: Array = list.get("lines", [])
	var rows: Array[Control] = RuleList.row_controls(rules)
	if rows.size() != lines.size():
		_fail("%d rows for %d lines" % [rows.size(), lines.size()])
		return
	for control in rows:
		if control.accessibility_name != String(control.get_meta("line")):
			_fail("a row's accessible name is not its line")
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
		print("[rule-list-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[rule-list-shot] %d lines drawn; OK" % rows.size())
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[rule-list-shot] FAILED: %s" % message)
	get_tree().quit(1)
