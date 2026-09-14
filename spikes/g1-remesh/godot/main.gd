# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Drives the measured run: N frames, a fixed vista camera, 20 explosions/s, one
# CSV row per frame, a screenshot, then quit. Everything that costs time lives
# in the extension; this script only reads monitors and writes text.
#
#   godot --path godot --rendering-driver vulkan -- \
#       --path=a --frames=900 --eps=20 --radius=4 --k=0 --b=0 \
#       --out=C:/.../results --tag=pathA
#
# NOT used, deliberately (plan §4): --fixed-fps and --write-movie, both of which
# force a fixed timestep and would destroy the quantity being measured.

extends Node3D

# `ext_dt_us` is the honest per-frame number: a raw monotonic `Instant` delta
# taken inside the extension. `frame_time_ms` is Godot's `_process(delta)`,
# which Godot is entitled to rewrite (the physics jitter fix does, and delta
# smoothing would if vsync were on). Both are disabled below, and the two series
# then agree to a few microseconds — but `analyse.py` still derives every
# frame-time percentile from `ext_dt_us`, so a future engine default cannot move
# a gate number without anyone noticing.
#
# `process_ms_1s_max` / `physics_ms_1s_max` are NOT per-frame, however they look
# sitting in a per-frame row: `Performance.TIME_PROCESS` and
# `TIME_PHYSICS_PROCESS` are refreshed once a second with the maximum seen in
# that second, so an 18 ms "process time" on a 1.4 ms frame is a stale
# per-second peak and not a contradiction. The names say so.
const CSV_HEADER := "frame,frame_time_ms,process_ms_1s_max,physics_ms_1s_max,draw_calls,primitives,video_mem_mb,queued_chunks,uploaded_chunks,uploaded_bytes,remesh_ms_this_frame,upload_ms_this_frame,explosions_this_frame,queued_after,lat_max_this_frame,ext_dt_us"

var opt := {
	"path": "a",
	# Plan §3 step 3: 3,600 frames = one minute at 60 fps. The short runs
	# measure a much lighter world — bytes per uploaded chunk more than double
	# between frame 900 and frame 3,600 as the craters accumulate — so 900 is
	# for exploratory sweeps only, never for a headline number.
	"frames": 3600,
	"eps": 20,
	"radius": 4,
	"k": 0,
	"b": 0,
	"seed": 1,
	"out": "",
	"tag": "",
	"flip_winding": "0",
	"shot": "-1",
}

var world: Node3D
var cam: Camera3D
var csv: FileAccess
var frame_no := 0
var target_frames := 3600
var shot_frame := -1
var shot_taken := false
var finished := false
var build_info := {}


func _parse_args() -> void:
	for a in OS.get_cmdline_user_args():
		var s := String(a)
		if s.begins_with("--"):
			s = s.substr(2)
		var kv := s.split("=", true, 1)
		if kv.size() == 2 and opt.has(kv[0]):
			opt[kv[0]] = kv[1]
		elif kv.size() == 2:
			push_warning("main.gd: unknown option %s" % kv[0])


func _ready() -> void:
	_parse_args()
	# Delta smoothing is OFF via project.godot's `application/run/delta_smoothing`.
	# It is read once at startup, and there is no GDScript setter for it in
	# 4.7.2: `Engine.set_delta_smoothing()` does not exist (the C++ `OS` method
	# is not exposed, and calling it is a *parse* error that kills the whole
	# script). So the script cannot set it — it verifies it, loudly, because a
	# smoothed `delta` snaps to refresh-rate quanta and every frame-time row
	# taken with it on is the smoother's output rather than a measurement.
	if bool(ProjectSettings.get_setting("application/run/delta_smoothing", true)):
		push_error("[g1] delta smoothing is ON: frame_time_ms will be a smoothed estimate, not a measurement. Set application/run/delta_smoothing=false in project.godot.")
	# The physics jitter fix is the *other* thing that rewrites `delta`: it makes
	# the process step consistent with whole physics ticks, which quantises the
	# reported delta to fractions of 1/physics_ticks_per_second. This spike runs
	# no physics at all, so it has nothing to fix here and only distorts the
	# number. Off, like vsync.
	Engine.physics_jitter_fix = 0.0
	target_frames = int(opt["frames"])
	shot_frame = int(opt["shot"])
	if shot_frame < 0:
		shot_frame = maxi(1, target_frames - 60)

	world = $World
	cam = $Camera3D

	# Fixed vista pose. Never moved during the run: two runs are only
	# comparable if the camera saw the same thing.
	cam.position = Vector3(192, 96, 24)
	cam.look_at(Vector3(192, 28, 240), Vector3.UP)
	cam.fov = 70.0
	cam.far = 2000.0

	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.42, 0.55, 0.72)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(1, 1, 1)
	env.ambient_light_energy = 1.0
	env.tonemap_mode = Environment.TONE_MAPPER_LINEAR
	cam.environment = env

	var path_id := 1 if String(opt["path"]).to_lower().begins_with("b") else 0
	world.configure(
		path_id,
		int(opt["k"]),
		int(opt["b"]),
		int(opt["eps"]),
		int(opt["radius"]),
		int(opt["seed"]),
		int(opt["flip_winding"]) != 0
	)
	world.set_camera_voxel(cam.position)
	build_info = world.build()

	var driver := String(ProjectSettings.get_setting("rendering/rendering_device/driver", "?"))
	print("[g1] adapter=%s api=%s driver_setting=%s" % [
		RenderingServer.get_video_adapter_name(),
		RenderingServer.get_video_adapter_api_version(),
		driver,
	])
	print("[g1] godot=%s path=%s frames=%d eps=%s K=%s B=%s radius=%s" % [
		Engine.get_version_info()["string"], opt["path"], target_frames,
		opt["eps"], opt["k"], opt["b"], opt["radius"],
	])
	print("[g1] build: ", build_info)

	var dir := String(opt["out"])
	if dir == "":
		dir = ProjectSettings.globalize_path("res://")
	DirAccess.make_dir_recursive_absolute(dir)
	var tag := String(opt["tag"])
	if tag == "":
		tag = "path%s_k%s_b%s" % [opt["path"], opt["k"], opt["b"]]
	csv = FileAccess.open("%s/frames_%s.csv" % [dir, tag], FileAccess.WRITE)
	if csv == null:
		push_error("main.gd: cannot open CSV in %s" % dir)
		get_tree().quit(2)
		return
	# The header carries the run's provenance so a CSV is never orphaned from
	# the machine it was taken on.
	# `timings=` says whether the frame numbers in this file mean anything: a
	# software rasteriser (the CI geometry job) renders correctly and times
	# nothing, so its CSV must not be mistaken for a measurement.
	var timings := "REAL_GPU"
	if OS.has_feature("movie") or DisplayServer.get_name() == "headless" or String(ProjectSettings.get_setting("rendering/rendering_device/driver", "")) == "dummy":
		timings = "MEANINGLESS_NO_GPU"
	csv.store_line("# godot=%s adapter=%s api=%s path=%s K=%s B=%s eps=%s radius=%s vsync=off delta_smoothing=off timings=%s" % [
		Engine.get_version_info()["string"],
		RenderingServer.get_video_adapter_name(),
		RenderingServer.get_video_adapter_api_version(),
		opt["path"], opt["k"], opt["b"], opt["eps"], opt["radius"], timings,
	])
	csv.store_line("# build=%s" % [build_info])
	csv.store_line(CSV_HEADER)


func _process(delta: float) -> void:
	if finished:
		return
	frame_no += 1
	var st: Dictionary = world.step()

	csv.store_line("%d,%.4f,%.4f,%.4f,%d,%d,%.2f,%d,%d,%d,%.4f,%.4f,%d,%d,%d,%d" % [
		frame_no,
		delta * 1000.0,
		Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0,
		Performance.get_monitor(Performance.TIME_PHYSICS_PROCESS) * 1000.0,
		int(Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME)),
		int(Performance.get_monitor(Performance.RENDER_TOTAL_PRIMITIVES_IN_FRAME)),
		Performance.get_monitor(Performance.RENDER_VIDEO_MEM_USED) / 1048576.0,
		int(st["queued_before"]),
		int(st["uploaded_chunks"]),
		int(st["uploaded_bytes"]),
		float(st["remesh_us"]) / 1000.0,
		float(st["upload_us"]) / 1000.0,
		int(st["explosions"]),
		int(st["queued_after"]),
		int(st["lat_max_this_frame"]),
		int(st["ext_dt_us"]),
	])

	if frame_no == shot_frame and not shot_taken:
		shot_taken = true
		_shoot()

	if frame_no >= target_frames:
		finished = true
		_finish()


func _shoot() -> void:
	# `frame_post_draw` never fires under the dummy rendering driver, so a
	# `--headless` run parks this coroutine for ever and writes no image at all —
	# silently, which is how a "blocking" CI geometry job can pass while checking
	# nothing. Anything that goes wrong here has to be loud.
	if DisplayServer.get_name() == "headless":
		push_error("[g1] --headless cannot render: no vista will be produced. Run windowed under a software rasteriser instead.")
		return
	await RenderingServer.frame_post_draw
	var tex := get_viewport().get_texture()
	if tex == null:
		push_error("[g1] viewport has no texture; no vista written")
		return
	var img := tex.get_image()
	if img == null or img.is_empty():
		push_error("[g1] viewport image is null/empty; no vista written")
		return
	var dir := String(opt["out"])
	if dir == "":
		dir = ProjectSettings.globalize_path("res://")
	var tag := String(opt["tag"])
	if tag == "":
		tag = String(opt["path"])
	var out_png := "%s/vista_%s.png" % [dir, tag]
	var err := img.save_png(out_png)
	if err != OK:
		push_error("[g1] save_png failed (%d) for %s" % [err, out_png])
		return
	print("[g1] screenshot written: %s (%dx%d)" % [out_png, img.get_width(), img.get_height()])


func _finish() -> void:
	var sm: Dictionary = world.summary()
	print("[g1] summary: ", sm)
	var dir := String(opt["out"])
	if dir == "":
		dir = ProjectSettings.globalize_path("res://")
	var tag := String(opt["tag"])
	if tag == "":
		tag = "path%s_k%s_b%s" % [opt["path"], opt["k"], opt["b"]]
	var f := FileAccess.open("%s/summary_%s.json" % [dir, tag], FileAccess.WRITE)
	if f != null:
		var merged := {}
		for k in build_info:
			merged["build_" + String(k)] = build_info[k]
		for k in sm:
			merged[String(k)] = sm[k]
		merged["adapter"] = RenderingServer.get_video_adapter_name()
		merged["api"] = RenderingServer.get_video_adapter_api_version()
		merged["godot"] = Engine.get_version_info()["string"]
		f.store_string(JSON.stringify(merged, "  "))
		f.close()
	if csv != null:
		csv.flush()
		csv.close()
		csv = null
	get_tree().quit(0)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST and csv != null:
		csv.flush()
		csv.close()
		csv = null
