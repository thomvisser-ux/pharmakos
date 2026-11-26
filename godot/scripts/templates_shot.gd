# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The template list, drawn with NO host running (S3's `prob`): a Probation match's first Lull,
# the guided first pick marked. `godot/fixtures/list_templates_guided.json` is a
# byte-identical copy of the gateway's golden (tests/golden/gateway/list_templates_guided/
# expected.response.json; crates/client-gdext/tests/godot_project.rs keeps the two identical).
# It goes through the bridge's own decode (`editor_templates_of_response`, the editor's
# `wizard::templates_of`) and the panel's own drawing (`scripts/editor.gd`'s static
# `fill_templates`), so the picture is the panel's picture of that answer: one button per
# template, the marked one tinted with the gateway's sentence under it, and no words of the
# client's on the mark.
#
# Without `--shot=` it is a check: every template has its button, exactly one is marked, and
# the sentence under it is the gateway's. NOT YET COMPARED: `cargo xtask screenshot` renders
# only the shots `xtask`'s SHOTS lists, and its row is an `xtask` change this lane does not own
# (S3's `cert3` adds it); until then it is run by hand (godot/README.md):
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/templates_shot.tscn --shot=<png>
#
# Exit codes: 0 when the list was drawn as sent (and shot, when asked) with no caught panic, 1
# when anything failed.

extends Control

const EditorPanel := preload("res://scripts/editor.gd")
const FIXTURE := "res://fixtures/list_templates_guided.json"
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var box: VBoxContainer = $Panel/Templates


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var templates: Array = bridge.editor_templates_of_response(FileAccess.get_file_as_string(FIXTURE))
	if templates.is_empty():
		_fail("the fixture did not decode; the reason is in the log")
		return
	EditorPanel.fill_templates(box, templates, true, {}, {}, func(_id: String) -> void: pass, [])
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var buttons := 0
	var marked: Array = []
	for template in templates:
		if template.get("guided", false):
			marked.append(template)
	for child in box.get_children():
		if child is Button and child.has_meta("id"):
			buttons += 1
	if buttons != templates.size():
		_fail("%d buttons for %d templates" % [buttons, templates.size()])
		return
	if marked.size() != 1:
		_fail("%d templates marked; the fixture marks one" % marked.size())
		return
	var drawn: Array = EditorPanel.guided_in(box)
	if drawn != [String(marked[0].get("id", "")), String(marked[0].get("guided_reason", ""))]:
		_fail("the mark is not drawn as sent: %s" % [drawn])
		return
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
		print("[templates-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[templates-shot] %d templates drawn, %s marked; OK" % [buttons, drawn[0]])
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[templates-shot] FAILED: %s" % message)
	get_tree().quit(1)
