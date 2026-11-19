# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The condition picker, drawn with NO host running (S3's `ed3b`, part 2, item 7): the render-only
# scene for the picker's screenshot, as `rule_list_shot.gd` is the rule list's.
#
# `godot/fixtures/vocabulary.json` is a `get_vocabulary` answer captured from a hosted match
# (godot/README.md says how and when); it goes through the bridge's own decode
# (`editor_vocabulary_of_response`) and the editor's own picker drawing
# (`scripts/condition_picker.gd`), so the picture is the editor's picture of that answer: each
# family with its predicates and their parameters, and the locked ones greyed with the gateway's
# reason beside them. The shot opens the first family that holds a locked entry, and the first
# predicate under it that can be picked, so one locked entry and its reason are on screen.
#
# NOT YET COMPARED: `cargo xtask screenshot` renders the shots `xtask`'s SHOTS lists, and adding
# this one is an `xtask` change this lane does not own; until one lands, the scene is run by hand
# (godot/README.md), and without `--shot=` it is a smoke test that draws the picker, checks a
# locked entry is drawn with its reason and cannot be picked, and quits.
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/picker_shot.tscn --shot=<png>
#
# Exit codes: 0 when the picker was drawn (and shot, when asked) with no caught panic, 1 when
# anything failed.

extends Control

const ConditionPicker := preload("res://scripts/condition_picker.gd")
const FIXTURE := "res://fixtures/vocabulary.json"
## Frames drawn before the frame that is read back, so the layout has settled.
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var line: Label = $Panel/Box/Line
@onready var tree: Tree = $Panel/Box/Tree


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var vocabulary: Dictionary = bridge.editor_vocabulary_of_response(FileAccess.get_file_as_string(FIXTURE))
	if vocabulary.is_empty():
		_fail("the fixture did not decode; the reason is in the log")
		return
	ConditionPicker.fill(tree, vocabulary)
	line.text = ConditionPicker.status_text(vocabulary)
	var locked := _open_a_locked_entry()
	if locked == null:
		_fail("the fixture holds no locked entry to show")
		return
	if locked.is_selectable(0):
		_fail("a locked entry can be picked")
		return
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var caught: Variant = bridge.caught_panics()
	# A count past what an int holds comes as the bridge's refusal, a Dictionary: a failure too.
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
		print("[picker-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[picker-shot] %d rows drawn, the locked one reads \"%s\"; OK" % [ConditionPicker.rows(tree).size(), locked.get_text(0)])
	get_tree().quit(0)


## Opens the first family that holds a locked predicate, and the first predicate under it that
## can be picked, so its parameters show; returns the locked predicate's row, or null.
func _open_a_locked_entry() -> TreeItem:
	for family in tree.get_root().get_children():
		var locked: TreeItem = null
		var open: TreeItem = null
		for entry in family.get_children():
			if entry.is_selectable(0):
				if open == null:
					open = entry
			elif locked == null:
				locked = entry
		if locked != null:
			family.collapsed = false
			if open != null:
				open.collapsed = false
				open.select(0)
			return locked
	return null


func _fail(message: String) -> void:
	printerr("[picker-shot] FAILED: %s" % message)
	get_tree().quit(1)
