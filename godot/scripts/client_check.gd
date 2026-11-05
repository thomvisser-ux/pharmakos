# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The headless client check: does the extension load, does the bridge marshal, and did it
# catch a panic doing it.
#
# This is T12's acceptance seen from inside the engine. Everything it asserts is already
# asserted by `cargo test`; what only this run can tell us is whether the cdylib LOADS —
# whether `PharmakosBridge` is a real class in this Godot rather than a placeholder. On a
# fresh checkout it is a placeholder unless `godot --headless --path godot --import` has
# run first, because a non-editor Godot run loads GDExtensions only from
# `res://.godot/extension_list.cfg`, which the editor writes when it scans the project and
# which is git-ignored. G1 lost four runs to that. `cargo xtask stage-client` runs the
# import before this scene, and `.github/workflows/ci.yml`'s client leg runs the pair.
#
# GDScript here is a VIEW and a REPORT and nothing else (AGENTS.md section 3 rule 4): it
# reads what the bridge returns and prints it. There is no rule, no arithmetic on $, kW or
# a duration, and no decision in this file — the one comparison it makes of the bridge's work is
# `caught_panics == 0`, which is the assertion itself.
#
# Since S3's `ed3a` it also draws the camera and the editor's panel at the owner's window, off
# screen in a SubViewport (`_check_the_view`), and measures them: walled floats and pixels, of
# the view and nothing else (decisions-log items 139 (2) and 155 (3)). `cargo xtask ci`'s
# `stage-client` step runs this scene, so these run locally as well as in CI.
#
# Exit codes: 0 when everything held, 1 when it did not. The CI leg reads that and nothing
# else, because a Godot run that prints a failure and exits 0 is the silent-catch problem
# wearing a different hat.

extends Node3D

## Item 54's rules rows, as canonical `gp.v1.RulesTable` JSON: the one pinned copy in
## godot/, shared with the vista (`scripts/mesher_rules.gd` says why it is a copy and
## which test pins it).
const MesherRules := preload("res://scripts/mesher_rules.gd")

## One chunk edge and one chunk's voxel count, as the mesher defines them.
##
## Not a rule and not a tuning value: the chunk edge is the 32 the whole design is written
## on (AGENTS.md section 1, "32 cubed copy-on-write chunks"), and these two names exist so
## the array sizes below say what they are. `crates/client-gdext`'s own tests assert the
## Rust side of the same pair.
const CHUNK_EDGE := 32
const CHUNK_VOLUME := 32768

## The floor the driven chunk is built at, and the square bitten out of its top layer.
##
## The bite is what makes the second upload a rebuild rather than a patch: it changes the
## geometry enough that the uploader's guards fire. Both branches have to happen for the
## engine side to be exercised rather than merely reached.
const FLOOR_DEPTH := 8
const BITE_FROM := 12
const BITE_TO := 20

## Full light, so the bake the mesher folds into the vertex colour is not zero everywhere.
const FULL_LIGHT := 15

## The vista and the editor, drawn off screen at the owner's window (S2's stage demo,
## decisions-log item 155 (3): about 1456 by 819). A SubViewport has the size it is given
## whatever the display server says, which a headless window may not (item 143 (2)).
const VistaScene := preload("res://scenes/vista.tscn")
const EditorPanel := preload("res://scripts/editor.gd")
const Lobby := preload("res://scripts/lobby.gd")
const InstanceLog := preload("res://scripts/instance_log.gd")
const Strings := preload("res://scripts/strings.gd")
const OWNERS_WINDOW := Vector2i(1456, 819)
## A map as wide as the golden seed's, in voxels (x, up, north).
const MAP_EXTENT := Vector3(384.0, 64.0, 384.0)
## A structure in the map's far corner, where the whole-map framing draws it smallest, in the
## sim's axes (x east, y north, z up).
const FAR_STRUCTURE := Vector3i(40, 40, 10)
## How wide a structure's marker must be on screen, in pixels, to be told from its neighbour,
## and how many wheel notches may get from the whole map to that. Item 155 (3) 4: the demo's
## base kit could not be told apart at any zoom it reached.
const READABLE_PIXELS := 120.0
const NOTCHES_TO_READABLE := 16
## How far the point under the cursor may drift on screen while the wheel zooms on it, in
## pixels.
const CURSOR_DRIFT_PIXELS := 2.0
## The least one notch must change the camera's distance by, as a share: "the scroll wheel
## barely moves the view".
const ONE_NOTCH_SHARE := 0.2
## Frames the panel's containers get to lay out before they are read.
const SETTLE_FRAMES := 5

func _ready() -> void:
	# G1 section 10.12: the physics jitter fix rewrites `delta` into a quantised estimate.
	# Nothing here is timed, but no measurement taken later may inherit a smoothed clock.
	# The authority is `project.godot`'s `[physics] common/physics_jitter_fix=0.0`, which
	# applies before any scene loads and which `godot_project.rs` asserts; this line is
	# belt and braces for a scene run with a project setting someone has edited.
	Engine.physics_jitter_fix = 0

	var failures: Array[String] = []

	# 1. The class exists. This is the check only a real Godot can make: if the import
	#    step did not run, `PharmakosBridge` is a placeholder and this is where it shows.
	if not ClassDB.class_exists("PharmakosBridge"):
		_fail("PharmakosBridge is not a registered class — the extension did not load. Run `godot --headless --path godot --import` first.")
		return
	var bridge := get_node_or_null("Bridge")
	if bridge == null:
		_fail("the scene has no Bridge node")
		return

	print("[client-check] upload path: %s" % bridge.upload_path())

	# 2. The rules table marshals, and the mesher's five parameters come out of it.
	var configured: Dictionary = bridge.configure(4, MesherRules.RULES_JSON)
	if not configured.get("configured", false):
		failures.append("configure() refused the rules table: %s" % configured.get("reason", ""))
	else:
		print("[client-check] rules: %s" % configured.get("rules", {}))
		print("[client-check] renderer attached: %s (%s)" % [
			configured.get("attached", false), configured.get("reason", "")])

	# 3. A gateway result decodes through the schema, and a field the schema does not
	#    declare is refused rather than stripped. The refusal prints a note, not an ERROR
	#    (F6, decisions-log item 126 (3)): it is the answer this probe asks for.
	var status: Variant = bridge.decode_result("get_status", '{"status":{"phase":"LULL","round":1}}')
	if typeof(status) != TYPE_DICTIONARY:
		failures.append("decode_result(get_status) did not return a dictionary")
	else:
		print("[client-check] get_status: %s" % status)
	var refused: Variant = bridge.decode_result("get_status", '{"status":{"phase":"LULL"},"nope":1}')
	if refused != null:
		failures.append("an unknown field was accepted; it must be refused with a pointer")

	# 4. The chunk transposition, on a length the mesher would refuse (a note, not an ERROR).
	var short := PackedByteArray()
	short.resize(16)
	if bridge.transpose_chunk(short).size() != 0:
		failures.append("a short chunk was padded instead of refused")

	# 5. The self-check itself: paths A and B over the same chunk set.
	var report: Dictionary = bridge.run_self_check()
	print("[client-check] self check: %s" % report)
	if not report.get("ok", false):
		failures.append("the self check failed: %s" % report.get("failures", ""))
	if not report.get("geometry_identical", false):
		failures.append("paths A and B produced different geometry")

	# 6. The engine side of the upload, which is the one thing in this file that
	#    `cargo test` cannot reach. Everything above runs identically with no engine in
	#    the loop — the self-check compares two PLAIN-DATA resident surfaces, not engine
	#    state. These calls are the only ones in the whole harness that reach a real
	#    `RenderingServer`: `mesh_create`, `mesh_add_surface_from_arrays`,
	#    `mesh_surface_set_material` with a RID taken from a material the renderer owns,
	#    `instance_create2`, the `mesh_get_surface` read-back that decides whether the
	#    in-place path is legal at all, and — where the read-back allows it — the region
	#    writes themselves.
	if configured.get("attached", false):
		_drive_the_engine_upload(bridge, failures)
	else:
		print("[client-check] no 3D world on this node, so the engine-side upload was not driven")

	# 6b. The vista and the panel, drawn at the owner's window (item 155 (3)).
	await _check_the_view(failures)

	# 7. The count the whole instrument exists for. gdext's catch at the `#[func]`
	#    boundary is silent, so a healthy-looking run with a non-zero count here is
	#    exactly the case this assertion is for. It is read AFTER the engine-side
	#    upload, so a panic inside a `RenderingServer` call is part of the verdict.
	var caught: int = bridge.caught_panics()
	print("[client-check] %s" % bridge.panic_report())
	if caught != 0:
		failures.append("%d panic(s) were caught at the bridge boundary" % caught)

	if failures.is_empty():
		print("[client-check] OK")
		get_tree().quit(0)
	else:
		for failure in failures:
			printerr("[client-check] FAILED: %s" % failure)
		get_tree().quit(1)

## Drives three real uploads through the bridge, so the engine side runs.
##
## Three calls over the same chunk index, in the order that reaches every branch the
## uploader has:
##
##   1. nothing is resident, so the plan is CREATE — and it is the create arm that reads
##      the surface layout back out of the engine, which is the probe item 53's in-place
##      write is only ever allowed on;
##   2. a bite is taken out of the floor's top layer, which moves quads between face
##      directions; measured, that changes the VERTEX COUNT, which is the uploader's first
##      rebuild guard, so the plan is REBUILD — `mesh_clear` and a fresh
##      `mesh_add_surface_from_arrays` over the same RID. (A change that kept the count but
##      moved the index array would fire the second guard instead; the counters say which
##      fired, and the run reports `rebuilds_vertex_count: 1`.)
##   3. the same bitten geometry again, so nothing moved and the plan is PATCH — if the
##      read-back said the layout is the one a region write assumes.
##
## A floor of a different DEPTH would not do for step 2: it has the same vertex count and
## the same index array as the first, so it patches, and the rebuild arm would never run.
## That is not a guess — the first version of this check used two depths and reported one
## create and two patches.
##
## MEASURED, not assumed: headless Godot 4.7.2 is **not** a no-op here. The first run of
## this check on CI's Linux leg reported `surface_probe: strides 12/4, colour truncate` and
## two patches, which means `mesh_get_surface` returned a real surface layout and the
## in-place region writes executed against the rendering server. The comment this replaces
## predicted the opposite — that the dummy renderer would return nothing and the probe would
## refuse the in-place path — and the measurement says otherwise, so the measurement is what
## is written down.
##
## The assertion is still on `rebuilds + patches >= 2` rather than on a patch count. A
## platform whose headless server returned no surface layout would refuse the in-place path
## and rebuild instead, and that is a correct outcome, not a failure: what this run exists to
## show is that the calls execute against a real engine without panicking. The byte-for-byte
## equality of what the two paths leave resident is `run_self_check`'s to prove.
func _drive_the_engine_upload(bridge: Node, failures: Array[String]) -> void:
	var light := PackedByteArray()
	light.resize(CHUNK_VOLUME)
	light.fill(FULL_LIGHT)

	var whole: PackedByteArray = bridge.transpose_chunk(_sim_chunk(false))
	var bitten: PackedByteArray = bridge.transpose_chunk(_sim_chunk(true))
	if whole.size() != CHUNK_VOLUME or bitten.size() != CHUNK_VOLUME:
		failures.append("transpose_chunk refused a full-length chunk")
		return

	var created: Dictionary = bridge.upload_chunk(0, Vector3.ZERO, whole, light)
	var changed: Dictionary = bridge.upload_chunk(0, Vector3.ZERO, bitten, light)
	var repeated: Dictionary = bridge.upload_chunk(0, Vector3.ZERO, bitten, light)
	for report in [created, changed, repeated]:
		if report.is_empty():
			failures.append("upload_chunk returned nothing; its reason is in the log above")
			return
	if int(created.get("vertices", 0)) <= 0:
		failures.append("the first engine-side upload produced no geometry")

	var counters: Dictionary = bridge.upload_counters()
	print("[client-check] engine upload counters: %s" % counters)
	print("[client-check] upload budget: %s" % bridge.upload_budget())
	if int(counters.get("creates", 0)) < 1:
		failures.append("the engine-side upload never created a surface")
	if int(counters.get("rebuilds", 0)) < 1:
		failures.append("the bitten chunk did not rebuild: %s" % counters)
	var settled: int = int(counters.get("rebuilds", 0)) + int(counters.get("patches", 0))
	if settled < 2:
		failures.append("the two later uploads neither rebuilt nor patched: %s" % counters)

## One chunk in SIM voxel order: a flat floor, optionally with a bite out of its top layer.
##
## Sim order is east + 32*north + 1024*up (decisions-log item 92), so a floor FLOOR_DEPTH
## layers deep is exactly the first FLOOR_DEPTH slabs of 1024 bytes. The bridge transposes
## it into the mesher's order; this script does not, because deciding the order is the
## bridge's job and duplicating it here is how the two would drift.
##
## `bite` clears a square of the top layer. That is what moves quads between face
## directions and so changes the index array — the difference between a rebuild and a
## patch, and the only reason there are two shapes here at all.
func _sim_chunk(bite: bool) -> PackedByteArray:
	var chunk := PackedByteArray()
	chunk.resize(CHUNK_VOLUME)
	chunk.fill(0)
	var layer := CHUNK_EDGE * CHUNK_EDGE
	for index in range(FLOOR_DEPTH * layer):
		chunk[index] = 1
	if bite:
		var top := (FLOOR_DEPTH - 1) * layer
		for north in range(BITE_FROM, BITE_TO):
			for east in range(BITE_FROM, BITE_TO):
				chunk[top + north * CHUNK_EDGE + east] = 0
	return chunk

func _fail(message: String) -> void:
	printerr("[client-check] FAILED: %s" % message)
	get_tree().quit(1)


## The camera and the panel at the owner's window, in a SubViewport (see OWNERS_WINDOW).
func _check_the_view(failures: Array[String]) -> void:
	var stage := SubViewport.new()
	stage.size = OWNERS_WINDOW
	stage.own_world_3d = true
	stage.render_target_update_mode = SubViewport.UPDATE_DISABLED
	add_child(stage)
	var vista: Node3D = VistaScene.instantiate()
	stage.add_child(vista)
	# Nothing here drains a view or eases a marker: this check moves them by hand.
	vista.set_process(false)
	vista.my_seat = "seat.0"
	_check_the_wheel(vista, failures)
	var editor: CanvasLayer = CanvasLayer.new()
	editor.set_script(EditorPanel)
	stage.add_child(editor)
	editor.setup(vista)
	await _check_the_panel(editor, failures)
	await _check_the_size_meter(editor, failures)
	_check_the_box(editor, failures)
	_check_escape(editor, failures)
	stage.queue_free()
	_check_the_recap_words(failures)
	_check_the_logs(failures)


## **The wheel reaches a readable structure** (item 155 (3) 4). From the whole map's framing,
## with the cursor on a structure in the far corner: one notch moves the view, the point under
## the cursor stays under it, and within NOTCHES_TO_READABLE notches the structure is
## READABLE_PIXELS wide. Measured through the real camera at the owner's window, not computed
## from the rig's constants.
func _check_the_wheel(vista: Node3D, failures: Array[String]) -> void:
	var rig: Node3D = vista.rig
	var camera: Camera3D = rig.camera
	rig.set_process(false)
	rig.frame_extent(MAP_EXTENT)
	var world := Vector3(FAR_STRUCTURE.x + 0.5, FAR_STRUCTURE.z, FAR_STRUCTURE.y + 0.5)
	# Level ground at the structure's feet, in place of a view this check has not fed. With no
	# ground answered the rig falls back on the plane through the focus: `_check_no_ground`.
	var ground := Plane(Vector3.UP, world)
	rig.ground_under = func(origin: Vector3, normal: Vector3) -> Dictionary:
		var met: Variant = ground.intersects_ray(origin, normal)
		return {"hit": met != null, "at": met}
	var whole := _structure_pixels(camera, world)
	var cursor := camera.unproject_position(world)
	if not Rect2(Vector2.ZERO, Vector2(OWNERS_WINDOW)).has_point(cursor):
		failures.append("the whole map's framing does not show the far structure: %s" % cursor)
		return
	print("[client-check] the whole map at %s: distance %.0f, the structure %.1f px wide at %s" % [OWNERS_WINDOW, rig.distance, whole, cursor])
	if whole >= READABLE_PIXELS:
		failures.append("the whole map already draws the structure %.1f px wide: the check below says nothing" % whole)
	var before: float = rig.distance
	_wheel_notch(rig, cursor, MOUSE_BUTTON_WHEEL_UP)
	if before - rig.distance < before * ONE_NOTCH_SHARE:
		failures.append("one wheel notch moved the camera from %.1f to %.1f, under %.0f%%" % [before, rig.distance, ONE_NOTCH_SHARE * 100.0])
	var notches := 1
	while _structure_pixels(camera, world) < READABLE_PIXELS and notches < NOTCHES_TO_READABLE:
		_wheel_notch(rig, cursor, MOUSE_BUTTON_WHEEL_UP)
		notches += 1
	var close := _structure_pixels(camera, world)
	var drift := camera.unproject_position(world).distance_to(cursor)
	print("[client-check] %d notch(es) on it: distance %.1f, the structure %.1f px wide, %.1f px from the cursor" % [notches, rig.distance, close, drift])
	if close < READABLE_PIXELS:
		failures.append("%d wheel notches left the structure %.1f px wide, under %.0f" % [notches, close, READABLE_PIXELS])
	if drift > CURSOR_DRIFT_PIXELS:
		failures.append("the wheel zoomed the structure %.1f px away from the cursor" % drift)
	# Its name is drawn at that distance, as the gateway spelt it, and not from the whole map.
	vista.take_view({"entities": [{"id": "s_1", "kind": "structure", "subtype": "autocannon", "owner": "seat.0", "at": FAR_STRUCTURE}]})
	var name_label: Label3D = vista.marker_node("s_1").get_node_or_null("Name")
	if name_label == null or name_label.text != "autocannon":
		failures.append("the structure's marker carries no name: %s" % name_label)
	elif name_label.visibility_range_end < rig.distance or name_label.visibility_range_end >= before:
		failures.append("the name is drawn out to %.0f voxels, with the camera at %.1f and the whole map at %.0f" % [name_label.visibility_range_end, rig.distance, before])
	# A notch out moves the camera the other way; a fractional notch by its fraction.
	var near_here: float = rig.distance
	_wheel_notch(rig, cursor, MOUSE_BUTTON_WHEEL_DOWN)
	if rig.distance <= near_here:
		failures.append("a notch out did not move the camera out: %.1f from %.1f" % [rig.distance, near_here])
	_check_no_ground(rig, failures)
	var precise := InputEventMouseButton.new()
	precise.factor = 0.25
	if not is_equal_approx(rig.notches_of(precise), 0.25):
		failures.append("a quarter of a notch is read as %s" % rig.notches_of(precise))
	precise.factor = 0.0
	if not is_equal_approx(rig.notches_of(precise), 1.0):
		failures.append("a wheel that reports no factor is read as %s notches" % rig.notches_of(precise))


## Over the sky, or before the view is whole, there is no ground to zoom on: the wheel still
## zooms, on the level plane through the focus.
func _check_no_ground(rig: Node3D, failures: Array[String]) -> void:
	rig.ground_under = func(_origin: Vector3, _normal: Vector3) -> Dictionary: return {"hit": false}
	var at := Vector2(OWNERS_WINDOW) * 0.5
	var before: float = rig.distance
	var pivot: Vector3 = rig._pivot_at(at)
	if not is_equal_approx(pivot.y, rig.focus.y):
		failures.append("with no ground the wheel zooms on %s, not the plane through %s" % [pivot, rig.focus])
	_wheel_notch(rig, at, MOUSE_BUTTON_WHEEL_UP)
	if rig.distance >= before:
		failures.append("with no ground the wheel did not zoom: %.1f from %.1f" % [rig.distance, before])


## One wheel notch at `at`, as the rig's `_unhandled_input` gets it.
func _wheel_notch(rig: Node3D, at: Vector2, button: MouseButton) -> void:
	var event := InputEventMouseButton.new()
	event.button_index = button
	event.pressed = true
	event.factor = 1.0
	event.position = at
	event.global_position = at
	rig._unhandled_input(event)


## How wide the vista's structure marker (three voxels across) is on screen, standing at `world`.
func _structure_pixels(camera: Camera3D, world: Vector3) -> float:
	var left := camera.unproject_position(world + Vector3(-1.5, 1.5, 0.0))
	var right := camera.unproject_position(world + Vector3(1.5, 1.5, 0.0))
	return left.distance_to(right)


## **The panel stays on screen** (item 139 (2) D, item 155 (3) 1): with the wizard open, a long
## refusal on the status line and rows with Fix buttons, the panel is its own width against the
## window's right edge, every control in it stays inside it, and a long sentence wraps.
func _check_the_panel(editor: CanvasLayer, failures: Array[String]) -> void:
	var long_words := "A sentence as long as the gateway may write one, with many ordinary words in it, so that it has to wrap inside the panel and may never run off the window's edge."
	var wizard := {
		"template_id": "a_template",
		"answered": true,
		"current": true,
		"why": long_words,
		"refusal": long_words,
		"pages": [{"pointer": "/seed", "label": long_words, "value": "{\"feature_id\":\"vent_150_13\"}", "suggested": true}],
	}
	var rows := [{"severity": "warning", "sentence": long_words, "code": "W0709", "pointer": "/declarative/route/0/place_beacon/at/voxel", "fixes": [{"title": long_words, "index": 0}]}]
	var state := {
		"changes": 1,
		"has_text": true,
		"status_key": "gateway_refused",
		"status_detail": long_words,
		"verdict": "quick",
		"rows": rows,
		"rows_current": true,
		"wizard": wizard,
		"templates": [{"id": "a_template", "title": long_words, "summary": long_words}],
		"prose": PackedStringArray([long_words]),
		"prose_current": true,
		"dormant": {"shown": true, "checked": false, "enabled": true},
		"this_round": long_words,
		"route": {"current": true, "reachable": true},
	}
	editor._draw_state(state)
	for _frame in SETTLE_FRAMES:
		await get_tree().process_frame
	var panel: Rect2 = editor.panel_rect()
	print("[client-check] the panel at %s: %s" % [OWNERS_WINDOW, panel])
	if not is_equal_approx(panel.end.x, float(OWNERS_WINDOW.x)) or not is_equal_approx(panel.size.x, EditorPanel.PANEL_WIDTH):
		failures.append("the panel is not its own width against the window's right edge: %s" % panel)
	for control in editor._panel.find_children("*", "Control", true, false):
		if not control.is_visible_in_tree():
			continue
		var rect: Rect2 = control.get_global_rect()
		if rect.position.x < panel.position.x - 0.5 or rect.end.x > panel.end.x + 0.5:
			failures.append("%s (%s) runs out of the panel: %s" % [control.get_class(), control.name, rect])
	if editor._status.get_line_count() < 2:
		failures.append("the status line's long sentence did not wrap")
	var fixes: Array = editor._rows.find_children("*", "Button", true, false)
	if fixes.is_empty() or fixes[0].size.y <= editor.panel_button("save").size.y:
		failures.append("a long Fix title did not wrap inside its button: %s" % [fixes])


## **The size meter** (S3's plan, E9): hidden with no budget; the verifier's numbers as they came;
## over budget the verifier's sentence; the bar's fill is the ProgressBar's own ratio, full past
## the budget.
func _check_the_size_meter(editor: CanvasLayer, failures: Array[String]) -> void:
	editor._draw_size({})
	if editor._size_box.visible:
		failures.append("the size meter is drawn with no budget named")
	editor._draw_size({"units": 32, "budget": 128, "refusal": ""})
	if not editor._size_box.visible or editor.size_text() != Strings.text("size_meter", {"units": 32, "budget": 128}):
		failures.append("the size meter says `%s`" % editor.size_text())
	if not is_equal_approx(editor._size_bar.ratio, 0.25):
		failures.append("the size bar's fill is %s at 32 of 128" % editor._size_bar.ratio)
	var said := "This playbook is larger than the seal allows."
	editor._draw_size({"units": 140, "budget": 128, "refusal": said})
	if editor.size_text() != said:
		failures.append("over budget the size meter says `%s`, not the verifier's sentence" % editor.size_text())
	if not is_equal_approx(editor._size_bar.ratio, 1.0):
		failures.append("over budget the size bar is not full: %s" % editor._size_bar.ratio)


## **The unticked box is drawn** (finding M): an outline a player can see, clear inside.
func _check_the_box(editor: CanvasLayer, failures: Array[String]) -> void:
	if not editor._dormant.has_theme_icon_override("unchecked"):
		failures.append("the dormant checkbox draws Godot's default box, which hardly shows")
	var box: Image = EditorPanel.unticked_box(EditorPanel.BOX_COLOUR)
	if box.get_pixel(0, 0).a < 0.9 or box.get_pixel(box.get_width() / 2, box.get_height() / 2).a > 0.0:
		failures.append("the unticked box is not an outline, clear inside")


## **Escape closes the map menus** (finding I), through the one function both input paths call.
## `watch_check.gd` sends the key through the engine's input on a live menu.
func _check_escape(editor: CanvasLayer, failures: Array[String]) -> void:
	var escape := InputEventKey.new()
	escape.keycode = KEY_ESCAPE
	escape.pressed = true
	if editor.close_menu_on_escape(escape):
		failures.append("Escape with no menu open was taken")
	# The key reaching the window itself. A menu that gets the key closes itself, as a headless
	# run measures (watch_check.gd's step passes with or without this path), so this is the path
	# only this check holds.
	editor._menu.clear()
	editor._menu.add_item("x", EditorPanel.ITEM_GO)
	editor._menu.popup()
	if not editor.menu_open():
		failures.append("the map menu did not open for the Escape check")
		return
	var other := InputEventKey.new()
	other.keycode = KEY_A
	other.pressed = true
	if editor.close_menu_on_escape(other) or not editor.menu_open():
		failures.append("a key that is not Escape closed the map menu")
	editor._input(escape)
	if editor.menu_open():
		failures.append("Escape reaching the window did not close the map menu")


## **Every seat's tally at the match's end** (item 155 (3) 7): the gateway's prose, then one frame
## a seat, each number as it came; between rounds, the prose alone.
func _check_the_recap_words(failures: Array[String]) -> void:
	var between := {"round": 2, "prose": "Words.", "seat_tallies": []}
	if Lobby.recap_text_of(between) != Strings.text("lobby_recap", {"round": 2, "prose": "Words."}):
		failures.append("the recap between rounds says `%s`" % Lobby.recap_text_of(between))
	var ended := {"round": 3, "prose": "Words.", "seat_tallies": [
		{"seat": "seat.0", "kills": 3, "shared_kills": 1, "value_destroyed_dollars": 60, "losses": 2},
		{"seat": "seat.1", "kills": 0, "shared_kills": 0, "value_destroyed_dollars": 0, "losses": 3},
	]}
	var words := Lobby.recap_text_of(ended)
	for seat in ended["seat_tallies"]:
		var line := Strings.text("lobby_recap_tally", {"seat": seat["seat"], "kills": seat["kills"], "shared_kills": seat["shared_kills"], "value": seat["value_destroyed_dollars"], "losses": seat["losses"]})
		if not words.contains(line):
			failures.append("the match's end does not say `%s`: `%s`" % [line, words])


## **Each instance keeps its own log** (finding N): a second logger starting leaves the first's
## file whole and still written, each process's path is its own, and the prune keeps KEPT.
func _check_the_logs(failures: Array[String]) -> void:
	var folder := "user://client_check_logs"
	DirAccess.make_dir_recursive_absolute(folder)
	var first_path := "%s/client-1.log" % folder
	var second_path := "%s/client-2.log" % folder
	var first := InstanceLog.new(first_path)
	first._log_message("first, before\n", false)
	var second := InstanceLog.new(second_path)
	second._log_message("second\n", false)
	first._log_message("first, after\n", false)
	first._log_error("f", "file", 1, "code", "said", false, 0, [])
	var first_says := FileAccess.get_file_as_string(first_path)
	var second_says := FileAccess.get_file_as_string(second_path)
	if not ("first, before" in first_says and "first, after" in first_says and "ERROR: said" in first_says) or "second" in first_says:
		failures.append("the first instance's log is not its own whole: `%s`" % first_says)
	if second_says.strip_edges() != "second":
		failures.append("the second instance's log is not its own: `%s`" % second_says)
	if InstanceLog.path_of(1) == InstanceLog.path_of(2):
		failures.append("two processes name one log file")
	first = null
	second = null
	for number in InstanceLog.KEPT + 2:
		FileAccess.open("%s/client-%d.log" % [folder, number + 10], FileAccess.WRITE).close()
	InstanceLog.prune(folder)
	var left := DirAccess.get_files_at(folder)
	if left.size() != InstanceLog.KEPT:
		failures.append("the prune left %d logs, not %d: %s" % [left.size(), InstanceLog.KEPT, left])
	for name in left:
		DirAccess.remove_absolute("%s/%s" % [folder, name])
	DirAccess.remove_absolute(folder)
