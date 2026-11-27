# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The beacon's ring, drawn with NO host running (S3's `prob`, the plan's E3): the vista over
# `godot/fixtures/view_built.jsonl`, a byte-identical copy of the gateway's `view_built` golden
# (tests/golden/gateway/view_built/expected.keyframe.jsonl; crates/client-gdext/tests/
# vista_fixture.rs keeps the two identical): seat 0's keyframe with a finished Resonance Spire
# standing on its core, so the core's ring is the grown one the gateway sent. The base ring is
# the opening keyframe's (`fixtures/view_keyframe.jsonl`, the vista shot's), which this scene
# checks too, before it draws the grown one. Each page goes through `view_apply`, the live rig's
# own decode, and the camera opens on the core.
#
# Without `--shot=` it is a check: the core's ring is drawn at the radius each view sent, and
# the scene quits 0.
# COMPARED: `cargo xtask screenshot` renders it and compares it with
# `tests/golden/vista/expected.ring.png` (`xtask`'s SHOTS, from S3's `cert3`). It can also
# be run by hand (godot/README.md):
#
#     godot --path godot --resolution 1280x720 -- --scene=res://scenes/ring_shot.tscn --shot=<png>
#
# Exit codes: 0 when both rings were drawn as sent (and shot, when asked) with no caught panic,
# 1 when anything failed.

extends Node

const BASE := "res://fixtures/view_keyframe.jsonl"
const GROWN := "res://fixtures/view_built.jsonl"
const FIXTURE_SEAT := "seat.0"
## The beacon the two fixtures give a ring: seat 0's core, as the gateway names it.
const CORE := "b_00"
const SETTLE_DRAWS := 4
const DRAIN_LIMIT := 2000

@onready var vista: Node3D = $Vista


func _ready() -> void:
	Engine.physics_jitter_fix = 0
	var shot := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--shot="):
			shot = arg.trim_prefix("--shot=")
	if shot != "" and DisplayServer.get_name() == "headless":
		_fail("a screenshot needs a window; run this windowed, not with --headless")
		return
	vista.my_seat = FIXTURE_SEAT
	vista.configure()
	var base := _apply(BASE)
	if base < 0:
		return
	var grown := _apply(GROWN)
	if grown < 0:
		return
	if base <= 0 or grown <= base:
		_fail("the core's ring is %d voxels in the opening view and %d once the Spire stands: not a base ring and a grown one" % [base, grown])
		return
	vista.settle_markers()
	for beacon in vista.beacons():
		if String(beacon["id"]) == CORE:
			vista.rig.map_extent = Vector3(vista.bridge.view_extent())
			vista.rig.open_on(beacon["position"], 0.0)
	var draws := 0
	while vista.pending != 0 or draws == 0:
		await get_tree().process_frame
		draws += 1
		if draws > DRAIN_LIMIT:
			_fail("the drain queue never emptied: %d chunks still waiting" % vista.pending)
			return
	for i in SETTLE_DRAWS:
		await get_tree().process_frame
	var caught: Variant = vista.bridge.caught_panics()
	if not (caught is int) or caught != 0:
		_fail("%s panic(s) were caught at the bridge boundary" % str(caught))
		return
	if shot != "":
		await RenderingServer.frame_post_draw
		var image := get_viewport().get_texture().get_image()
		var error := image.save_png(shot)
		if error != OK:
			_fail("the PNG could not be written to %s (error %d)" % [shot, error])
			return
		print("[ring-shot] %dx%d written to %s" % [image.get_width(), image.get_height(), shot])
	print("[ring-shot] the core's ring: %d voxels at the base, %d grown; OK" % [base, grown])
	get_tree().quit(0)


## Feeds every page of `path` through the bridge and the vista; answers the core's ring as
## drawn, or -1 after a failure.
func _apply(path: String) -> int:
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		_fail("%s could not be opened" % path)
		return -1
	var complete := false
	while not file.eof_reached():
		var line := file.get_line()
		if line.strip_edges() == "":
			continue
		var view: Dictionary = vista.bridge.view_apply(line)
		if view.is_empty():
			_fail("a page of %s did not decode; the reason is in the log" % path)
			return -1
		vista.take_view(view)
		complete = view.get("complete", false)
	if not complete:
		_fail("%s ended before its view completed" % path)
		return -1
	return vista.ring_of(CORE)


func _fail(message: String) -> void:
	printerr("[ring-shot] FAILED: %s" % message)
	get_tree().quit(1)
