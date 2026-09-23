# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The vista golden's scene: the committed keyframe fixture, drawn with NO host running.
#
# `godot/fixtures/view_keyframe.jsonl` is a byte-identical copy of the gateway's own
# keyframe golden (seat 0, the opening Lull of the golden seed, one served `get_view`
# result per line; `crates/client-gdext/tests/vista_fixture.rs` keeps it identical). Each
# line goes through `view_apply`, the same decode entry the live watch rig uses, so the
# picture is the live client's picture of that keyframe: the whole generated map (decisions
# log item 107 (1)), and seat 0's own beacon and units standing on it.
#
# The shot is taken on a frame OUTSIDE any measured series (skeleton plan T16, acceptance):
# nothing in this scene is timed, the drain queue is emptied first, a few frames are drawn
# with nothing left to upload, and only then is the frame read back. G1 measured 67-91 ms
# on a shot frame; that frame is spent here and nowhere a number is taken.
#
# `--shot=<png>` names where the PNG goes. It is taken windowed - `cargo xtask screenshot`
# runs under xvfb - because `godot --headless` selects the dummy renderer, where
# `frame_post_draw` never fires and a screenshot parks for ever (G1 section 10.12). Without
# `--shot=` the scene draws the map, checks itself and quits, which is a smoke test that
# does run headless.
#
# Exit codes: 0 when the map was drawn (and shot, when asked) with no caught panic, 1 when
# anything failed.

extends Node

const FIXTURE := "res://fixtures/view_keyframe.jsonl"
## The seat the fixture was served to.
const FIXTURE_SEAT := "seat.0"
## Frames drawn with nothing left to upload before the frame that is read back: enough for
## the last upload to be on screen and for no shot to land on an upload frame.
const SETTLE_DRAWS := 4
## Frames the drain may take before the shot gives up. 288 chunks at K = 4 a frame is 72.
const DRAIN_LIMIT := 2000

@onready var vista: Node3D = $Vista


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this under xvfb, not with --headless")
		return
	_draw_and_shoot(shot)


func _draw_and_shoot(shot: String) -> void:
	vista.my_seat = FIXTURE_SEAT
	vista.configure()
	var file := FileAccess.open(FIXTURE, FileAccess.READ)
	if file == null:
		_fail("%s could not be opened" % FIXTURE)
		return
	var complete := false
	var pages := 0
	while not file.eof_reached():
		var line := file.get_line()
		if line.strip_edges() == "":
			continue
		var view: Dictionary = vista.bridge.view_apply(line)
		if view.is_empty():
			_fail("line %d of the fixture did not decode; the reason is in the log" % (pages + 1))
			return
		vista.take_view(view)
		complete = view.get("complete", false)
		pages += 1
	if not complete:
		_fail("the fixture ended before its view completed")
		return
	vista.settle_markers()
	vista.frame_whole_map()

	var draws := 0
	while vista.pending != 0 or draws == 0:
		await get_tree().process_frame
		draws += 1
		if draws > DRAIN_LIMIT:
			_fail("the drain queue never emptied: %d chunks still waiting" % vista.pending)
			return
	for i in SETTLE_DRAWS:
		await get_tree().process_frame

	var caught: int = vista.bridge.caught_panics()
	print("[vista-shot] %d page(s), %d markers, drained over %d draws; %s" % [
		pages, vista.marker_count(), draws, vista.bridge.panic_report()])
	if caught != 0:
		_fail("%d panic(s) were caught at the bridge boundary" % caught)
		return
	if shot != "":
		await RenderingServer.frame_post_draw
		var image := get_viewport().get_texture().get_image()
		var error := image.save_png(shot)
		if error != OK:
			_fail("the PNG could not be written to %s (error %d)" % [shot, error])
			return
		print("[vista-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[vista-shot] OK")
	get_tree().quit(0)


func _fail(message: String) -> void:
	printerr("[vista-shot] FAILED: %s" % message)
	get_tree().quit(1)
