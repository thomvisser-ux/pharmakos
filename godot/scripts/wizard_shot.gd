# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The template wizard's first page, drawn with NO host running: T19 pull request 2's
# render-only scene (skeleton-plan-w6-notes.md section A4 item 2).
#
# `godot/fixtures/instantiate_suggested.json` is a byte-identical copy of the gateway's
# `instantiate_suggested` golden (crates/client-gdext/tests/wizard.rs keeps it one): seat
# 0's `instantiate_template{suggested: true}` answer on the golden seed. It goes through the
# bridge's own decode (`editor_wizard_of_response`) and the editor's own page drawing
# (`scripts/wizard.gd`), so the picture is the editor's picture of that answer: the first
# page with its label, its raw value in the field and the operator's mark, the why as it
# came, and Use and Close.
#
# RENDER-ONLY: the shot is taken and looked at, and NOT compared or committed. Decisions-log
# item 110 (4) and the w6 notes' A5 book the render-only mode's comparison to T20, which
# commits this shot as a golden beside the rows shot.
#
# `--shot=<png>` names where the PNG goes; it is taken windowed, never `--headless`, for the
# same reason as the vista's shot (scripts/vista_shot.gd). Without `--shot=` the scene draws
# the page, checks the accessible names, and quits: a smoke test that runs headless.
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/wizard_shot.tscn --shot=<png>
#
# Exit codes: 0 when the page was drawn (and shot, when asked) with no caught panic, 1 when
# anything failed.

extends Control

const Wizard := preload("res://scripts/wizard.gd")
const Strings := preload("res://scripts/strings.gd")
const FIXTURE := "res://fixtures/instantiate_suggested.json"
## Frames drawn before the frame that is read back, so the layout has settled.
const SETTLE_DRAWS := 4

@onready var bridge: Node = $Bridge
@onready var pages: VBoxContainer = $Panel/Pages


func _ready() -> void:
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	var instance: Dictionary = bridge.editor_wizard_of_response(FileAccess.get_file_as_string(FIXTURE))
	if instance.is_empty() or (instance.get("pages", []) as Array).is_empty():
		_fail("the fixture did not decode; the reason is in the log")
		return
	# The first page, as the wizard shows it: the answer as it came, cut to its first page.
	var first := instance.duplicate()
	first["pages"] = [(instance["pages"] as Array)[0]]
	first["answered"] = true
	first["current"] = true
	first["refusal"] = ""
	# No heading: the live editor heads the wizard with `list_templates`' title, which a
	# hostless scene does not have, and the scene reads nothing out of the playbook itself.
	var title := ""
	var on_send := func(_pointer: String, _text: String) -> void: pass
	var on_press := func() -> void: pass
	Wizard.fill(pages, title, first, on_send, on_press, on_press)
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var drawn: Array[Control] = Wizard.page_controls(pages)
	if drawn.size() != 1:
		_fail("%d pages were drawn, not the first one" % drawn.size())
		return
	var page: Dictionary = first["pages"][0]
	var control: Control = drawn[0]
	if control.accessibility_name != String(page.get("label", "")):
		_fail("the page's accessible name is not its label")
		return
	if (control.get_meta("field") as LineEdit).accessibility_name != String(page.get("label", "")):
		_fail("the value field's accessible name is not its label")
		return
	var mark: Label = control.get_meta("mark")
	if mark.visible != bool(page.get("suggested", false)) or mark.accessibility_name != mark.text:
		_fail("the mark is not drawn where the answer sets it, or is not named by its words")
		return
	if Wizard.why_text(pages) != Strings.text("wizard_why", {"why": instance.get("why", "")}):
		_fail("the why is not drawn as it came")
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
		print("[wizard-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[wizard-shot] the first of %d page(s) drawn; OK" % (instance["pages"] as Array).size())
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[wizard-shot] FAILED: %s" % message)
	get_tree().quit(1)
