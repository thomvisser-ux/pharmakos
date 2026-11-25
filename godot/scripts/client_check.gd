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
const LobbyScene := preload("res://scenes/lobby.tscn")
const InstanceLog := preload("res://scripts/instance_log.gd")
const ValueFieldScript := preload("res://scripts/value_field.gd")
const Wizard := preload("res://scripts/wizard.gd")
const Values := preload("res://scripts/values.gd")
const Rows := preload("res://scripts/rows.gd")
const Strings := preload("res://scripts/strings.gd")
const HostLink := preload("res://scripts/host_link.gd")
## The worst view page `crates/client-gdext/tests/transport.rs` builds, in bytes: the same page
## is built here and received through host_link.gd's own peer.
const WORST_PAGE_BYTES := 1230868
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
## A low look across the map, where the plane under the focus runs out before the horizon.
const LOW_PITCH := -0.1
## The structure kinds as the gateway spells them (`crates/gateway/src/view.rs`), the base kit's
## anchor spacing in voxels, the one kind that stands apart from the kit (one to a beacon, S3's
## plan E4), and the least height a name may be read at, in pixels.
const KIT := ["generator", "autocannon", "mortar", "survey_post", "resonance_spire", "wall"]
## The kit's names as the gateway's view titles them (`ViewEntity.title`, the `view_built` golden),
## which the vista draws in place of the subtype (S3's `fixq-c`).
const KIT_TITLES := ["Generator", "Autocannon", "Mortar", "Survey post", "Resonance Spire", "Wall"]
const ANCHOR_SPACING := 4.0
const LONE_KIND := "resonance_spire"
const NAME_PIXELS_TALL := 10.0
## The S2 demo playbook's warnings, about as many rows as it opens, and the project's own window
## height, the smallest the panel is drawn at.
const OPEN_ROWS := 6
const SMALL_WINDOW_HEIGHT := 720.0
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
	# A count past what an int holds comes as the bridge's refusal, a Dictionary: a failure too.
	var caught: Variant = bridge.caught_panics()
	print("[client-check] %s" % bridge.panic_report())
	if not (caught is int) or caught != 0:
		failures.append("%s panic(s) were caught at the bridge boundary" % str(caught))

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
	# The editor's popups are windows embedded in the stage, as the game window embeds them, so
	# input pushed into the stage reaches them as the window's input reaches them.
	stage.gui_embed_subwindows = true
	add_child(stage)
	var vista: Node3D = VistaScene.instantiate()
	stage.add_child(vista)
	# Nothing here drains a view or eases a marker: this check moves them by hand.
	vista.set_process(false)
	vista.my_seat = "seat.0"
	_check_the_wheel(vista, failures)
	await _check_the_name(vista, failures)
	_check_the_looks(vista, failures)
	var editor: CanvasLayer = CanvasLayer.new()
	editor.set_script(EditorPanel)
	stage.add_child(editor)
	editor.setup(vista)
	await _check_the_panel(editor, failures)
	await _check_the_size_meter(editor, failures)
	_check_the_box(editor, failures)
	_check_escape(editor, failures)
	await _check_an_action_pick_closes_the_menus(stage, editor, failures)
	await _check_the_entry_list(stage, editor, failures)
	_check_the_popups_are_opaque(editor, failures)
	await _check_the_build_marks(editor, failures)
	await _check_the_dormant_box(stage, editor, failures)
	_check_the_load_folder(editor, failures)
	_check_the_unreadable_counts(editor, failures)
	await _check_the_tall_entry_list(stage, editor, failures)
	await _check_the_rig_backed_panel(stage, editor, failures)
	await _check_the_value_field(stage, editor, failures)
	_check_the_refused_ghost_line(editor, failures)
	stage.queue_free()
	_check_the_recap_words(failures)
	await _check_the_lobby_after_a_failure(failures)
	await _check_the_remembered_line(failures)
	_check_the_logs(failures)
	await _check_the_playtest_lobby(failures)
	await _check_playtest_off_costs_nothing(failures)
	await _check_a_lost_release_counts_the_next_turn(failures)
	await _check_a_bad_link_keeps_the_rig(failures)
	await _check_export_after_a_stopped_match(failures)


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
	# Its name is drawn at that distance, as the gateway titled it, and not from the whole map.
	vista.take_view({"entities": [{"id": "s_1", "kind": "structure", "subtype": "autocannon", "owner": "seat.0", "at": FAR_STRUCTURE, "title": "Autocannon", "look": {"shape": "tower", "x": 1, "y": 1, "z": 1}}]})
	var name_label: Label3D = vista.marker_node("s_1").get_node_or_null("Name")
	if name_label == null or name_label.text != "Autocannon":
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
## zooms, on the level plane through the focus where the ray meets it over the map. Just below
## the horizon the ray meets that plane far off the map, and there the wheel zooms on the focus,
## which a notch must not throw off the map. Walks the screen's middle column up to find both.
func _check_no_ground(rig: Node3D, failures: Array[String]) -> void:
	rig.ground_under = func(_origin: Vector3, _normal: Vector3) -> Dictionary: return {"hit": false}
	rig.frame_extent(MAP_EXTENT)
	rig.pitch = LOW_PITCH
	rig._place()
	var over_map := Vector2(-1, -1)
	var off_map := Vector2(-1, -1)
	var plane := Plane(Vector3.UP, rig.focus)
	var column := OWNERS_WINDOW.x / 2
	for row in range(OWNERS_WINDOW.y - 1, -1, -1):
		var at := Vector2(column, row)
		var met: Variant = plane.intersects_ray(rig.camera.project_ray_origin(at), rig.camera.project_ray_normal(at))
		if met == null:
			continue
		if rig._over_the_map(met):
			over_map = at
		elif over_map.y >= 0.0:
			off_map = at
			break
	if over_map.y < 0.0 or off_map.y < 0.0:
		failures.append("the low camera found no row over the map (%s) or past it (%s)" % [over_map, off_map])
		return
	var pivot: Vector3 = rig._pivot_at(over_map)
	if not is_equal_approx(pivot.y, rig.focus.y) or not rig._over_the_map(pivot):
		failures.append("over the map with no ground the wheel zooms on %s" % pivot)
	if rig._pivot_at(off_map) != rig.focus:
		failures.append("past the map the wheel zooms on %s, not the focus %s" % [rig._pivot_at(off_map), rig.focus])
	var before: float = rig.distance
	var kept: Vector3 = rig.focus
	_wheel_notch(rig, off_map, MOUSE_BUTTON_WHEEL_UP)
	if rig.distance >= before:
		failures.append("with no ground the wheel did not zoom: %.1f from %.1f" % [rig.distance, before])
	if rig.focus != kept:
		failures.append("a notch just below the horizon threw the focus from %s to %s" % [kept, rig.focus])
	print("[client-check] the low camera: row %d is over the map, row %d is past it, and a notch there leaves the focus at %s" % [over_map.y, off_map.y, rig.focus])
	rig.frame_extent(MAP_EXTENT)
	rig.ground_under = Callable()
	var unframed := Vector3.ZERO
	rig.map_extent = unframed
	if rig._pivot_at(Vector2(OWNERS_WINDOW) * 0.5) != rig.focus:
		failures.append("with no map known the wheel zooms on %s, not the focus" % rig._pivot_at(Vector2(OWNERS_WINDOW) * 0.5))
	rig.map_extent = MAP_EXTENT


## **A structure's name fits its neighbour's and can be read** (item 155 (3) 4), for the kit's
## names as the gateway spells them: measured off the label itself, with the base kit's anchors
## ANCHOR_SPACING voxels apart, and, for the height, through the camera at the name's farthest
## drawn distance. PNG goldens are not this check's: how it looks is P6a's hand check.
func _check_the_name(vista: Node3D, failures: Array[String]) -> void:
	var entities := []
	for index in KIT.size():
		entities.append({"id": "s_%d" % index, "kind": "structure", "subtype": KIT[index], "owner": "seat.0", "at": Vector3i(100 + index * 4, 100, 10), "title": KIT_TITLES[index], "look": {"shape": "block", "x": 1, "y": 1, "z": 1}})
	vista.take_view({"entities": entities})
	vista.settle_markers()
	await get_tree().process_frame
	await get_tree().process_frame
	var rig: Node3D = vista.rig
	rig.frame_extent(MAP_EXTENT)
	for index in KIT.size():
		var label: Label3D = vista.marker_node("s_%d" % index).get_node("Name")
		# A billboard's box is as tall as it is wide, whichever way it turns, so its width is the
		# label's and its height is not: that is the font's line, read through the camera.
		var box: AABB = label.get_aabb()
		var line: float = ThemeDB.fallback_font.get_height(label.font_size) * label.pixel_size
		rig.focus = label.global_position
		rig.distance = label.visibility_range_end
		rig._place()
		var bottom: Vector2 = rig.camera.unproject_position(label.global_position)
		var top: Vector2 = rig.camera.unproject_position(label.global_position + rig.camera.global_basis.y * line)
		var pixels: float = bottom.distance_to(top)
		print("[client-check] the name `%s`: %.2f voxels wide, %.1f px tall at %.0f voxels off" % [KIT_TITLES[index], box.size.x, pixels, label.visibility_range_end])
		if box.size.x <= 0.0:
			failures.append("the name `%s` has no size: %s" % [KIT[index], box])
		elif KIT[index] != LONE_KIND and box.size.x > ANCHOR_SPACING:
			failures.append("the name `%s` is %.2f voxels wide, over its neighbour's at %.0f" % [KIT[index], box.size.x, ANCHOR_SPACING])
		if pixels < NAME_PIXELS_TALL:
			failures.append("the name `%s` is %.1f px tall at its farthest, under %.0f" % [KIT[index], pixels, NAME_PIXELS_TALL])
	vista.take_view({"entities": []})


## **Each structure is drawn by its look and named by its title** (S3's `fixq-c`, items 1 to 3;
## P6b finding 8: every structure was one cube named by its subtype). The gateway's `view_built`
## golden, through its byte copy and the bridge's own decoding: the autocannon is a tower and the
## mortar a dish, so they draw different meshes; the mortar going up is named in the gateway's
## going-up words and drawn see-through; the wall is a slab of its box, named "Wall". Then another
## seat's structure at half its hit points: its plain title, a partial bar, nothing see-through,
## because going up is the gateway's word and never read from hit points.
func _check_the_looks(vista: Node3D, failures: Array[String]) -> void:
	vista.configure()
	var built: Array = []
	for path in ["res://fixtures/view_keyframe.jsonl", "res://fixtures/view_built.jsonl"]:
		for line in FileAccess.get_file_as_string(path).split("\n", false):
			var view: Dictionary = vista.bridge.view_apply(line)
			if view.is_empty():
				failures.append("a page of %s did not decode" % path)
				return
			vista.take_view(view)
			if view.has("entities"):
				built = view["entities"]
	var body := func(id: String) -> MeshInstance3D:
		var node: Node3D = vista.marker_node(id)
		return node.get_node_or_null("Body") if node != null else null
	var name_of := func(id: String) -> String:
		var node: Node3D = vista.marker_node(id)
		var label: Label3D = node.get_node_or_null("Name") if node != null else null
		return label.text if label != null else ""
	var cannon: MeshInstance3D = body.call("s_3")
	var mortar: MeshInstance3D = body.call("s_4")
	var wall: MeshInstance3D = body.call("l_1")
	if cannon == null or mortar == null or wall == null:
		failures.append("the built view drew no autocannon, mortar or wall marker")
		return
	var tower := cannon.mesh as CylinderMesh
	var dish := mortar.mesh as CylinderMesh
	if tower == null or dish == null or tower.top_radius != tower.bottom_radius or dish.top_radius == dish.bottom_radius:
		failures.append("the autocannon (a tower) and the mortar (a dish) are not drawn as an upright and a bowl: %s, %s" % [cannon.mesh, mortar.mesh])
		failures.append("the autocannon (a tower) and the mortar (a dish) draw the same mesh: %s, %s" % [cannon.mesh, mortar.mesh])
	if not (wall.mesh is BoxMesh) or (wall.mesh as BoxMesh).size != Vector3(4, 2, 1):
		failures.append("the wall is not drawn as its 4 by 1 by 2 slab: %s" % [wall.mesh])
	for named in [["s_3", "Autocannon"], ["s_4", "Mortar, going up"], ["l_1", "Wall"], ["s_2", "Survey post"]]:
		if name_of.call(named[0]) != named[1]:
			failures.append("%s is named `%s`, not the gateway's `%s`" % [named[0], name_of.call(named[0]), named[1]])
	var paint: StandardMaterial3D = mortar.mesh.material
	if paint.transparency == BaseMaterial3D.TRANSPARENCY_DISABLED:
		failures.append("the mortar the gateway says is going up is drawn as standing")
	# Another seat's survey post, hit to half: plain, standing, a partial bar.
	var hit: Array = []
	# A fresh id, so the marker is drawn anew from this entity (fix pass 1, item 7: a redraw of the
	# same marker would not run `_marker_for`, and going up read from hit points would pass).
	for entity in built:
		var one: Dictionary = entity.duplicate()
		if String(one["id"]) == "s_2":
			one["id"] = "s_2_hit"
			one["hp"] = 100
		hit.append(one)
	vista.take_view({"entities": hit})
	var post: MeshInstance3D = body.call("s_2_hit")
	var fill: MeshInstance3D = vista.marker_node("s_2_hit").get_node_or_null("Bar/Fill")
	if name_of.call("s_2_hit") != "Survey post" or (post.mesh.material as StandardMaterial3D).transparency != BaseMaterial3D.TRANSPARENCY_DISABLED:
		failures.append("another seat's structure at half its hit points is named `%s` or drawn see-through" % name_of.call("s_2_hit"))
	if fill == null or (fill.mesh as QuadMesh).size.x >= vista.BAR_SIZE.x:
		failures.append("another seat's structure at half its hit points has no partial bar")
	print("[client-check] the built view: tower, dish and slab drawn by look; titles as sent; going up see-through")
	vista.take_view({"entities": []})


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
	editor._draw_size({}, true, "")
	if editor._size_box.visible:
		failures.append("the size meter is drawn with no budget named")
	editor._draw_size({"units": 32, "budget": 128, "refusal": ""}, true, "")
	if not editor._size_box.visible or editor.size_text() != Strings.text("size_meter", {"units": 32, "budget": 128}):
		failures.append("the size meter says `%s`" % editor.size_text())
	if not is_equal_approx(editor._size_bar.ratio, 0.25):
		failures.append("the size bar's fill is %s at 32 of 128" % editor._size_bar.ratio)
	var said := "This playbook is larger than the seal allows."
	editor._draw_size({"units": 140, "budget": 128, "refusal": said}, true, "")
	if editor.size_text() != said:
		failures.append("over budget the size meter says `%s`, not the verifier's sentence" % editor.size_text())
	if not is_equal_approx(editor._size_bar.ratio, 1.0):
		failures.append("over budget the size bar is not full: %s" % editor._size_bar.ratio)
	# A report on an older text: the meter says what the verdict's line says for that wait, and
	# neither its count nor its refusal.
	var waiting := Strings.text("verdict_checking")
	editor._draw_size({"units": 140, "budget": 128, "refusal": said}, false, waiting)
	if editor.size_text() != waiting or editor._size_bar.modulate.a >= 1.0:
		failures.append("the size meter says `%s` of an older text" % editor.size_text())
	# Through the panel's own redraw: rows that describe an older text, rows the gateway
	# refused, and a Load refused, which leaves the text on screen the one that was counted.
	var size := {"units": 32, "budget": 128, "refusal": ""}
	var counted := Strings.text("size_meter", {"units": 32, "budget": 128})
	var refused := Strings.text("unchecked_refused", {"detail": "INTERNAL: no"})
	for case in [
		[{"has_text": true, "rows_current": false, "verdict": "quick", "size": size}, waiting],
		[{"has_text": true, "rows_current": false, "verdict": "quick", "size": size, "unchecked": {"rows": {"key": "refused", "detail": "INTERNAL: no"}}}, refused],
		[{"has_text": true, "rows_current": false, "verdict": "refused", "size": size}, counted],
		[{"has_text": true, "rows_current": true, "verdict": "quick", "size": size}, counted],
	]:
		editor._draw_state(case[0])
		if editor.size_text() != case[1]:
			failures.append("the panel's redraw of %s says `%s`, not `%s`" % [case[0], editor.size_text(), case[1]])
	# In sight with the rows open, at the top of a window smaller than the owner's.
	var rows := []
	for index in OPEN_ROWS:
		rows.append({"severity": "warning", "sentence": "A warning with enough words in it to take up the two lines most of them do on this panel.", "code": "W0709", "pointer": "/declarative/route/%d" % index, "fixes": []})
	editor._draw_state({"has_text": true, "rows_current": true, "verdict": "quick", "size": size, "rows": rows})
	for _frame in SETTLE_FRAMES:
		await get_tree().process_frame
	print("[client-check] the size meter ends %.0f px down" % editor.size_rect().end.y)
	if editor.size_rect().end.y > SMALL_WINDOW_HEIGHT:
		failures.append("with %d rows open the size meter ends at %.0f px, under a %.0f px window" % [OPEN_ROWS, editor.size_rect().end.y, SMALL_WINDOW_HEIGHT])
	var with_meter: float = editor._rows.get_global_rect().position.y
	editor._draw_state({"has_text": true, "rows_current": true, "verdict": "quick", "rows": rows})
	for _frame in SETTLE_FRAMES:
		await get_tree().process_frame
	if not is_equal_approx(editor._rows.get_global_rect().position.y, with_meter):
		failures.append("the size meter pushes the rows from %.0f px to %.0f" % [editor._rows.get_global_rect().position.y, with_meter])
	print("[client-check] with %d rows open the rows start %.0f px down, with the meter or without it" % [OPEN_ROWS, with_meter])


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


## **One value field for every typed value** (S3's `fixp`, P6b findings 1 and 2), driven as a
## person drives it, through the engine's input path: a click into a field holding "40", the keys
## "3" and "0", and a click on Send (or Enter) give one `sent` carrying "30". At P6b the SpinBox
## sent 40, or "4030". The second open after a use selects the old value on the first click too.
## The same through the editor's own popup, and through a wizard page.
func _check_the_value_field(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	var holder := Control.new()
	holder.position = Vector2(40, 40)
	stage.add_child(holder)
	var field: HBoxContainer = ValueFieldScript.new("Send")
	holder.add_child(field)
	var heard: Array[String] = []
	field.sent.connect(func(text: String) -> void: heard.append(text))
	field.open("40", "%")
	await _settle()
	await _type_into(stage, field.field, "30")
	_click(stage, field.send.get_global_rect().get_center())
	if heard != ["30"]:
		failures.append("the value field's click, keys and Send sent %s, not [\"30\"]" % [heard])
	field.open("40", "%")
	await _settle()
	await _type_into(stage, field.field, "30")
	_key(stage, KEY_ENTER, 0)
	if heard != ["30", "30"]:
		failures.append("a second open, keys and Enter sent %s, not a second \"30\"" % [heard])
	if field.field.text != "30":
		failures.append("the field shows `%s` after typing over \"40\"" % field.field.text)
	# Tab's focus, keys, then a click to move the caret, then a key: the click places the caret
	# and selects nothing, so the typed text is kept (fix pass 1, item 2).
	field.open("40", "%")
	await _settle()
	field.field.grab_focus()
	await get_tree().process_frame
	for character in "12":
		_key(stage, OS.find_keycode_from_string(character), character.unicode_at(0))
	await _type_into(stage, field.field, "3")
	_key(stage, KEY_ENTER, 0)
	if heard != ["30", "30", "123"]:
		failures.append("Tab, \"12\", a click and \"3\" sent %s, not a third \"123\" (the field shows `%s`)" % [heard, field.field.text])
	if field.unit.text != "%" or not field.unit.visible:
		failures.append("the unit is not drawn beside the field as sent: `%s`" % field.unit.text)
	holder.queue_free()

	# The editor's popup, through the popup's own window.
	var sent: Array[String] = []
	var listen := func(text: String) -> void: sent.append(text)
	editor._number_field.sent.connect(listen)
	editor._open_number({"index": 0, "revision": 0, "pointer": "/x"}, {"has_min": true, "min": 0, "has_max": true, "max": 100, "value_text": "40", "value_json": "40", "unit": "%", "display_unit": "%", "text": "40"})
	await _settle()
	var popup: PopupPanel = editor._number_popup
	if not popup.visible:
		failures.append("the number popup did not open")
	else:
		# Through the stage, which hands the input to the popup embedded in it, as the game window
		# does: a click into the field, the keys, a click on "Set this value"; then Enter.
		var inside := Vector2(popup.position)
		_click(stage, inside + editor._number_field.field.get_global_rect().get_center())
		for character in "30":
			_key(stage, OS.find_keycode_from_string(character), character.unicode_at(0))
		await get_tree().process_frame
		_click(stage, inside + editor._number_field.send.get_global_rect().get_center())
		if sent != ["30"]:
			failures.append("the editor's number popup sent %s, not [\"30\"] (the field shows `%s`)" % [sent, editor._number_field.field.text])
		_key(stage, KEY_ENTER, 0)
		if sent != ["30", "30"]:
			failures.append("Enter in the editor's number popup sent %s" % [sent])
		if editor._number_note.text != Strings.text("rule_number_bounds", {"min": 0, "max": 100, "unit": " %"}):
			failures.append("the number popup's note says `%s`, not the gateway's bounds" % editor._number_note.text)
	editor._number_field.sent.disconnect(listen)
	popup.hide()

	# A wizard page.
	var pages := VBoxContainer.new()
	pages.position = Vector2(40, 40)
	pages.custom_minimum_size = Vector2(380, 0)
	stage.add_child(pages)
	var asked: Array = []
	Wizard.fill(pages, "a template", {"answered": true, "current": true, "pages": [{"pointer": "/seed", "label": "Seed", "value": "40", "suggested": true}]}, func(pointer: String, text: String) -> void: asked.append([pointer, text]), func() -> void: pass, func() -> void: pass)
	await _settle()
	var page: Control = Wizard.page_controls(pages)[0]
	await _type_into(stage, page.get_meta("field"), "30")
	_click(stage, (page.get_meta("send") as Button).get_global_rect().get_center())
	if asked != [["/seed", "30"]]:
		failures.append("the wizard page's click, keys and Send asked %s, not [[\"/seed\", \"30\"]]" % [asked])
	pages.queue_free()


## A click into `field` as a person makes it, then `text` typed key by key: the old value goes.
func _type_into(viewport: Viewport, field: LineEdit, text: String) -> void:
	_click(viewport, field.get_global_rect().get_center())
	for character in text:
		_key(viewport, OS.find_keycode_from_string(character), character.unicode_at(0))
	await get_tree().process_frame


## A left click at `at` in `viewport`'s coordinates: the mouse moves there, presses and releases.
func _click(viewport: Viewport, at: Vector2) -> void:
	var motion := InputEventMouseMotion.new()
	motion.position = at
	motion.global_position = at
	viewport.push_input(motion, true)
	for down in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = MOUSE_BUTTON_LEFT
		event.pressed = down
		event.button_mask = MOUSE_BUTTON_MASK_LEFT if down else 0
		event.position = at
		event.global_position = at
		viewport.push_input(event, true)


## One key going down and up in `viewport`.
func _key(viewport: Viewport, keycode: Key, unicode: int) -> void:
	for down in [true, false]:
		var event := InputEventKey.new()
		event.keycode = keycode
		event.unicode = unicode
		event.pressed = down
		viewport.push_input(event, true)


func _settle() -> void:
	for _frame in SETTLE_FRAMES:
		await get_tree().process_frame


## **An action pick closes the map menus** (S3's `fixp`, P6b finding 3): a priority row and an
## enum setting's value in the Visit & change submenus, and a target choice, each reached and
## picked through the engine's input path, keys pushed into the stage, which hands them to the
## menu that holds the focus: Right opens a submenu as hovering does, Enter picks the row in focus
## (both through `PopupMenu`'s own activation, which runs Godot's hide-on-pick rule for the menu
## and its parents). After an action no map menu is open; a target choice leaves the beacon menu
## open, as the P6a fix asks.
func _check_an_action_pick_closes_the_menus(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	var beacon := {"id": "b_00", "owner": "seat.0", "position": Vector3.ZERO}
	var detail := {"known": true, "id": "b_00", "priority": "normal", "settings": [{"title": "Rebuild", "pointer": "/build/rebuild_destroyed", "value_json": "true", "editor": "choice", "values": PackedStringArray(["true", "false"]), "titles": PackedStringArray(["yes", "no"])}], "targets": []}
	for case in ["priority", "setting", "target"]:
		# A frame between one menu's close and the next click, as a person's hand takes.
		await get_tree().process_frame
		editor._on_beacon_clicked(beacon, false, Vector2(300, 300))
		editor._visit_drawn = ""
		editor._fill_visit_menu({"beacon_detail": detail, "vocabulary": {}})
		await get_tree().process_frame
		var menu: PopupMenu = editor._menu
		if case == "target":
			await _pick(stage, menu, menu.get_item_index(EditorPanel.ITEM_SELECTOR), KEY_ENTER, failures)
			if not menu.visible:
				failures.append("a target choice closed the beacon menu")
			editor.close_menu()
			continue
		await _pick(stage, menu, _submenu_row(menu, editor._visit_menu), KEY_RIGHT, failures)
		if case == "priority":
			await _pick(stage, editor._visit_menu, editor._visit_menu.get_item_index(EditorPanel.ITEM_LOW), KEY_ENTER, failures)
		else:
			var settings: PopupMenu = editor._visit_submenus[0]
			await _pick(stage, editor._visit_menu, _submenu_row(editor._visit_menu, settings), KEY_RIGHT, failures)
			var values: PopupMenu = settings.get_child(0)
			await _pick(stage, settings, _submenu_row(settings, values), KEY_RIGHT, failures)
			await _pick(stage, values, 1, KEY_ENTER, failures)
		if editor.menu_open():
			failures.append("%s picked left a map menu open" % ("a priority row" if case == "priority" else "an enum setting's value"))
		editor.close_menu()


## Row `index` of `menu` put in focus, and `key` pushed into the stage; a menu that is not open
## for it is a failure.
func _pick(stage: SubViewport, menu: PopupMenu, index: int, key: Key, failures: Array[String]) -> void:
	if not menu.visible or index < 0:
		failures.append("a map menu was not open for its pick (row %d)" % index)
		return
	menu.set_focused_item(index)
	_key(stage, key, 0)
	await get_tree().process_frame


## The row of `menu` that opens `submenu`, or -1.
func _submenu_row(menu: PopupMenu, submenu: PopupMenu) -> int:
	for index in menu.item_count:
		if menu.get_item_submenu_node(index) == submenu:
			return index
	return -1


## **Lists of entries are drawn in a list that fits** (S3's `fixp`, P6b finding 5): the
## vocabulary's blueprints, from the gateway's committed answer, open beside the menu they were
## picked from, at most PANEL_WIDTH wide and inside the window, with the locked Resonance Spire's
## sentence under it; a click on that row (pushed into the stage) picks nothing, and a click on an
## open row picks it. At P6b a PopupMenu, which does not wrap, ran wider than the window.
func _check_the_entry_list(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	var words: Dictionary = editor.bridge.editor_vocabulary_of_response(FileAccess.get_file_as_string("res://fixtures/vocabulary.json"))
	var blueprints: Array = words.get("blueprints", [])
	if blueprints.is_empty():
		failures.append("the vocabulary fixture listed no blueprint")
		return
	var picked: Array = []
	var listen := func(row: String, which: int) -> void: picked.append([row, which])
	editor.entry_picked.connect(listen)
	# Beside a menu near the window's right edge, where the old submenu ran off it.
	editor.open_entry_list(EditorPanel.ITEM_LIST_ADD_TARGET, blueprints, Rect2(Vector2(OWNERS_WINDOW.x - 200, 200), Vector2(180, 300)))
	await _settle()
	var popup: PopupPanel = editor._entry_popup
	var rect := Rect2(Vector2(popup.position), Vector2(popup.size))
	print("[client-check] the entry list: %s, %d rows" % [rect, editor.entry_rows().size()])
	if rect.size.x > EditorPanel.PANEL_WIDTH or not Rect2(Vector2.ZERO, Vector2(OWNERS_WINDOW)).encloses(rect):
		failures.append("the entry list %s is wider than %d or outside the window" % [rect, EditorPanel.PANEL_WIDTH])
	var spire: Control = null
	var open_row: Control = null
	for row in editor.entry_rows():
		var entry: Dictionary = blueprints[int(row.get_meta("entry"))]
		if String(entry.get("name", "")) == "resonance_spire":
			spire = row
		elif String(entry.get("locked", "")) == "" and open_row == null:
			open_row = row
	if spire == null or not spire.has_meta("locked") or not String(spire.get_meta("locked")).contains(String(blueprints[int(spire.get_meta("entry"))].get("locked", "?"))):
		failures.append("the locked Resonance Spire's row does not show its sentence")
	else:
		_click(stage, rect.position + (spire.get_child(0) as Control).get_global_rect().get_center())
		await get_tree().process_frame
		if not picked.is_empty():
			failures.append("a click on the locked Spire picked %s" % [picked])
	if open_row != null:
		_click(stage, rect.position + (open_row.get_child(0) as Control).get_global_rect().get_center())
		await get_tree().process_frame
		if picked != [["add_target", int(open_row.get_meta("entry"))]]:
			failures.append("a click on an open entry picked %s" % [picked])
	editor.entry_picked.disconnect(listen)
	popup.hide()


## **Popups draw an opaque panel** (S3's `fixp`, P6b finding 6): every popup the editor owns
## resolves its `panel` to a style that draws its centre at alpha 1. Measured on Godot 4.7.2 the
## default was a StyleBoxFlat at alpha 0.6, through which the map showed.
func _check_the_popups_are_opaque(editor: CanvasLayer, failures: Array[String]) -> void:
	var fallback: StyleBox = ThemeDB.get_default_theme().get_stylebox("panel", "PopupPanel")
	print("[client-check] Godot's own popup panel: %s, centre %s" % [fallback.get_class(), fallback.bg_color if fallback is StyleBoxFlat else "-"])
	for popup in editor.popups():
		var panel: StyleBox = popup.get_theme_stylebox("panel")
		if not (panel is StyleBoxFlat) or not panel.draw_center or panel.bg_color.a < 1.0:
			failures.append("%s draws a panel that is not opaque: %s" % [popup.get_class(), panel])


## **Build targets are marked on the map in the Lull** (S3's `fixp`, P6b finding 7): after the
## panel's redraw, one mark per target whose anchor can be drawn (a voxel), a wall's box outlined,
## and a line per target; an `on` vent the map does not list and an anchor the walk cannot read are
## said and not marked.
func _check_the_build_marks(editor: CanvasLayer, failures: Array[String]) -> void:
	var targets := [
		{"step": 1, "blueprint": "autocannon", "anchor": "voxel", "at": Vector3i(40, 41, 5), "has_box": false},
		{"step": 2, "blueprint": "wall", "anchor": "voxel", "at": Vector3i(10, 11, 4), "has_box": true, "box_min": Vector3i(10, 11, 3), "box_max": Vector3i(14, 11, 7)},
		{"step": 3, "blueprint": "generator", "anchor": "on", "feature": "vent_120_88", "has_box": false},
		{"step": 4, "blueprint": "mortar", "anchor": "unknown", "has_box": false},
	]
	editor._draw_state({"changes": 7, "has_text": true, "build_targets": targets})
	await get_tree().process_frame
	var marks: Array[Node] = editor.build_marks()
	if marks.size() != 2:
		failures.append("%d Build target marks for two drawable anchors" % marks.size())
	elif (marks[0] as Label3D).text != Strings.text("build_mark", {"step": 1, "blueprint": "autocannon"}):
		failures.append("the first Build mark says `%s`" % (marks[0] as Label3D).text)
	if (editor._build_boxes.mesh as ImmediateMesh).get_surface_count() != 1:
		failures.append("the wall's box is not outlined")
	var lines: PackedStringArray = editor.build_text().split("\n")
	if lines.size() != 4 or lines[2] != Strings.text("build_on_unlisted", {"step": 3, "blueprint": "generator", "feature": "vent_120_88"}) or lines[3] != Strings.text("build_unknown", {"step": 4, "blueprint": "mortar"}):
		failures.append("the Build target lines say `%s`" % editor.build_text())
	editor._draw_state({"changes": 8, "has_text": false})
	await get_tree().process_frame
	if not editor.build_marks().is_empty() or editor.build_text() != Strings.text("build_none"):
		failures.append("the Build marks outlived their text")


## **The dormant box: no click is lost without a word** (S3's `fixp`, P6b finding 9): the box
## shown and enabled, two press-and-release pairs at one point with no motion between them, pushed
## into the stage, give two `toggled`; a refusal the bridge names is said beside the box; and only
## the shut door disables it.
func _check_the_dormant_box(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	editor._draw_state({"changes": 9, "has_text": true, "rows_current": true, "verdict": "quick", "dormant": {"shown": true, "checked": false, "enabled": true, "refused": ""}})
	await _settle()
	var box: CheckBox = editor._dormant
	var toggles: Array = []
	var listen := func(on: bool) -> void: toggles.append(on)
	box.toggled.connect(listen)
	var at := box.get_global_rect().get_center()
	var motion := InputEventMouseMotion.new()
	motion.position = at
	motion.global_position = at
	stage.push_input(motion, true)
	for _pair in 2:
		for down in [true, false]:
			var event := InputEventMouseButton.new()
			event.button_index = MOUSE_BUTTON_LEFT
			event.pressed = down
			event.button_mask = MOUSE_BUTTON_MASK_LEFT if down else 0
			event.position = at
			event.global_position = at
			stage.push_input(event, true)
		await get_tree().process_frame
	box.toggled.disconnect(listen)
	if toggles.size() != 2:
		failures.append("two clicks at one point on the dormant box gave %d toggled: %s" % [toggles.size(), toggles])
	if not editor.dormant_log(at).contains("inside true"):
		failures.append("the dormant box's log line does not place the press inside it: %s" % editor.dormant_log(at))
	editor._draw_state({"changes": 10, "has_text": true, "rows_current": true, "verdict": "quick", "dormant": {"shown": true, "checked": false, "enabled": true, "refused": "busy"}})
	if not editor._dormant_said.visible or editor._dormant_said.text != Strings.text("status_dormant_refused_busy"):
		failures.append("a refused click is not said beside the box: `%s`" % editor._dormant_said.text)
	editor._draw_state({"changes": 11, "has_text": false})


## **Load opens where the text's own file is** (S3's `fixp`): the folder of the text's file, or
## else of the last file Loaded or Saved this match.
func _check_the_load_folder(editor: CanvasLayer, failures: Array[String]) -> void:
	var folder := ProjectSettings.globalize_path("res://fixtures")
	editor.aim_load({"file": folder.path_join("needs_a_fix.jsonc"), "last_file": "C:/elsewhere/x.jsonc"})
	if editor._load_dialog.current_dir.trim_suffix("/") != folder.trim_suffix("/"):
		failures.append("Load opens in `%s`, not the text's file's folder `%s`" % [editor._load_dialog.current_dir, folder])
	var other := ProjectSettings.globalize_path("res://scripts")
	editor.aim_load({"file": "", "last_file": other.path_join("x.jsonc")})
	if editor._load_dialog.current_dir.trim_suffix("/") != other.trim_suffix("/"):
		failures.append("with no file of its own Load opens in `%s`, not the last file's folder `%s`" % [editor._load_dialog.current_dir, other])
	if EditorPanel.load_folder({}) != "":
		failures.append("with no file at all Load names a folder: `%s`" % EditorPanel.load_folder({}))


## **No integer reader can fail at run time** (S3's `fixp`, the `godot_int` sweep's GDScript
## half): the bridge's refusal of a count past `int`, `{"not_an_int": "<digits>"}`, fed to each
## drawing path that reads one, is said as unreadable on its line, and no script error is raised.
func _check_the_unreadable_counts(editor: CanvasLayer, failures: Array[String]) -> void:
	var refusal := {"not_an_int": "18446744073709551615"}
	var digits := "18446744073709551615"
	if Values.int_of(7) != 7 or Values.int_of(refusal) is int:
		failures.append("Values.int_of does not tell a count from the bridge's refusal")
	editor._draw_meter({"answers": refusal})
	if not editor.meter_text().contains(digits):
		failures.append("the meter says `%s` of an unreadable answer count" % editor.meter_text())
	var chip := EditorPanel.chip_line({"step": refusal, "now": {}, "failure": "no_target", "matched": 0})
	if not chip.contains(digits):
		failures.append("a chip says `%s` of an unreadable step" % chip)
	editor._draw_attack([{"aim": "selector", "step": refusal}])
	if not editor._attack_label.text.contains(digits):
		failures.append("an Attack line says `%s` of an unreadable step" % editor._attack_label.text)
	editor._draw_build([{"step": refusal, "blueprint": "wall", "anchor": "unknown", "has_box": false}])
	if not editor.build_text().contains(digits):
		failures.append("a Build line says `%s` of an unreadable step" % editor.build_text())
	if not EditorPanel.picking_on({"picking": refusal}):
		failures.append("an unreadable picking index does not leave the click to the bridge")
	editor._draw_state({"changes": 12, "has_text": true, "undo_depth": refusal, "picking": refusal, "rows_current": true, "verdict": "quick", "rows": [{"severity": "warning", "sentence": "A row.", "code": "W0709", "pointer": "/x", "fixes": [{"title": "Fix it", "index": refusal}]}], "rule_list": {"lines": [{"segments": [{"text": "A line.", "chip": refusal}], "rule": refusal, "in_rule": -1}], "rules": [], "chips": [], "slots": []}, "prose_current": true, "rules_rendering": 3})
	if editor._undo_button.disabled or not editor._undo_button.tooltip_text.contains(digits):
		failures.append("an unreadable undo depth is not said on Undo: `%s`" % editor._undo_button.tooltip_text)
	var fix_said := false
	for label in editor._rows.find_children("*", "Label", true, false):
		if (label as Label).text.contains(digits):
			fix_said = true
	if not fix_said or not Rows.fix_buttons(editor._rows).is_empty():
		failures.append("a fix with an unreadable number is not said as unreadable, or is a button")
	var rule_said := false
	for label in editor._rules_box.find_children("*", "Label", true, false):
		if (label as Label).text.contains(digits):
			rule_said = true
	if not rule_said:
		failures.append("the rule list does not say an unreadable index")
	editor._draw_state({"changes": 13, "has_text": false})


## **The rig-backed checks** (fix pass 1, items 1 and 5), through the bridge's own rig and editor:
## a value field's refusal said in its popup, one field's answer kept from the next field, the
## dormant box's click while its edit is on its way, the Ready line after an accepted seal, and
## Load's folder from its own button.
func _check_the_rig_backed_panel(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	var bridge: Node = editor.bridge
	if not await _rig_begin(bridge):
		failures.append("the rig-backed editor never held its text with its rule list rendered: %s" % bridge.editor_state().get("status_key", ""))
		return
	await _check_the_field_refusal_and_its_owner(stage, editor, bridge, failures)
	await _check_the_dormant_box_through_the_rig(stage, editor, bridge, failures)
	await _check_the_planning_wire(stage, editor, bridge, failures)
	await _check_the_ready_line_through_the_rig(bridge, failures)
	await _check_load_through_its_button(stage, editor, bridge, failures)
	await _check_the_transport(bridge, failures)
	_check_a_close_on_a_message_is_said(bridge, failures)
	_check_the_saves(editor, bridge, failures)
	_check_the_limit_is_named(bridge, failures)


## A value field's "abc" goes to the gateway as typed (S3's `fixq-c`: the gateway reads typed
## numbers), and its refusal is said in the popup in the gateway's words. Then chip A
## sends 500 and, before the gateway answers, the person opens field B, a Visit & change number
## setting (the rule list takes no edit while A's is on its way), and types in it: A's refusal,
## and A's landing, are not B's to say, and B stays open with what was typed. Last, an edit of
## another control re-renders the list under an open chip field: it stays open, its text kept.
func _check_the_field_refusal_and_its_owner(stage: SubViewport, editor: CanvasLayer, bridge: Node, failures: Array[String]) -> void:
	var pct := _chip_index(bridge, "/cmdr_hp_pct/pct")
	if pct < 0:
		failures.append("the rendered rule list has no number chip to edit")
		return
	bridge.editor_describe_beacon("b_00")
	if not await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("beacon_detail", {}).get("known", false) and state.get("rules_editable", false)):
		failures.append("the rig-backed editor never read b_00's detail with its rule list editable")
		return
	var settings: Array = bridge.editor_state().get("beacon_detail", {}).get("settings", [])
	var setting := -1
	for index in settings.size():
		if String(settings[index].get("editor", "")) == "number":
			setting = index
	if setting < 0:
		failures.append("b_00's detail lists no number setting")
		return
	editor._visit_beacon = "b_00"
	var popup: PopupPanel = editor._number_popup
	editor._on_rule_chip(pct)
	await _settle()
	await _type_in_popup(stage, editor, "abc")
	_click(stage, Vector2(popup.position) + editor._number_field.send.get_global_rect().get_center())
	await _settle()
	await _rig_until(bridge, func(state: Dictionary) -> bool: return String(state.get("field", {}).get("state", "")) == "refused")
	await _settle()
	if not popup.visible or editor._number_said.text != Strings.text("rule_refused", {"detail": "INVALID_ARGUMENT: `abc` is not a number"}):
		failures.append("\"abc\" in the value field's popup says `%s` (open %s), not the gateway's refusal" % [editor._number_said.text, popup.visible])
	if _typed.is_empty() or _typed[-1] != "abc":
		failures.append("\"abc\" did not go to the gateway as typed: %s" % [_typed])
	for landed in [false, true]:
		if not await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false)):
			failures.append("the rule list never took an edit again")
			return
		editor._on_rule_chip(pct)
		await _settle()
		await _type_in_popup(stage, editor, "500")
		_click(stage, Vector2(popup.position) + editor._number_field.send.get_global_rect().get_center())
		if not await _rig_until(bridge, func(_state: Dictionary) -> bool: return not _held.is_empty(), func(method: String) -> bool: return method == "set_value_text"):
			failures.append("chip A's edit never went out")
			return
		# Field B opens before the gateway answers A, and the person types in it.
		editor._on_setting(setting)
		await _settle()
		await _type_in_popup(stage, editor, "7")
		_release(bridge, "" if landed else _refusal("INVALID_ARGUMENT", "out of range"))
		await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("prose_current", false) and not state.get("busy", true))
		await _settle()
		if not popup.visible or editor._number_field.field.text != "7" or editor._number_said.visible:
			failures.append("A's %s reached field B: open %s, `%s`, said `%s`" % ["landing" if landed else "refusal", popup.visible, editor._number_field.field.text, editor._number_said.text])
		# A frame between a popup's close and its next open, as a hand takes: a popup opened in
		# the frame it closed closes itself again.
		popup.hide()
		await _settle()
	# An open chip field, and another control's edit landing under it: the list re-renders, and the
	# field stays open on the chip at the same pointer with what was typed in it.
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	editor._on_rule_chip(pct)
	await _settle()
	await _type_in_popup(stage, editor, "9")
	var before := int(bridge.editor_state().get("rules_rendering", -1))
	editor.act("go", {"beacon": "b_00"})
	await _rig_until(bridge, func(state: Dictionary) -> bool: return int(state.get("rules_rendering", -1)) != before and state.get("prose_current", false) and not state.get("busy", true))
	await _settle()
	if not popup.visible or editor._number_field.field.text != "9" or int(editor._number_chip.get("revision", -1)) == before:
		failures.append("a re-render under an open chip field closed it or kept the old rendering: open %s, `%s`" % [popup.visible, editor._number_field.field.text])
	popup.hide()


## The value field's popup: a click into its field, then `text` key by key, through the stage.
func _type_in_popup(stage: SubViewport, editor: CanvasLayer, text: String) -> void:
	var inside := Vector2(editor._number_popup.position)
	_click(stage, inside + editor._number_field.field.get_global_rect().get_center())
	for character in text:
		_key(stage, OS.find_keycode_from_string(character), character.unicode_at(0))
	await get_tree().process_frame


## E0601 on the rows: the box is shown and takes a click; while that click's edit is on its way a
## second click at the same point reaches the bridge, which refuses it as busy, said beside the
## box. Reaches the bridge's `dormant()` and `set_allow_dormant` through the rig.
func _check_the_dormant_box_through_the_rig(stage: SubViewport, editor: CanvasLayer, bridge: Node, failures: Array[String]) -> void:
	_diagnostics = "{\"code\":\"E0601\",\"severity\":\"error\",\"path\":\"/declarative/route/0\",\"beginner\":\"Short of power.\",\"suggestions\":[{\"title\":\"Allow dormant beacons\",\"json_patch\":\"[{\\\"op\\\":\\\"add\\\",\\\"path\\\":\\\"/declarative/options\\\",\\\"value\\\":{\\\"allow_dormant_beacons\\\":true}}]\",\"applicability\":\"maybe_incorrect\"}]}"
	bridge.editor_load(RIG_TEXT.to_utf8_buffer())
	if not await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rows_current", false) and state.get("dormant", {}).get("shown", false)):
		failures.append("E0601 never showed the dormant box through the rig")
		return
	await _settle()
	var box: CheckBox = editor._dormant
	var toggles: Array = []
	var listen := func(on: bool) -> void: toggles.append(on)
	box.toggled.connect(listen)
	var at := box.get_global_rect().get_center()
	_click(stage, at)
	await _rig_until(bridge, func(_state: Dictionary) -> bool: return not _held.is_empty(), func(method: String) -> bool: return method == "patch_plan")
	await _settle()
	_click(stage, at)
	await _settle()
	box.toggled.disconnect(listen)
	if toggles.size() != 2:
		failures.append("two clicks on the dormant box, the second while the first's edit was on its way, gave %d toggled" % toggles.size())
	if String(bridge.editor_state().get("dormant", {}).get("refused", "")) != "busy" or editor._dormant_said.text != Strings.text("status_dormant_refused_busy"):
		failures.append("the click while the edit was on its way is not said beside the box: `%s`" % editor._dormant_said.text)
	_release(bridge, "")
	await _rig_until(bridge, func(state: Dictionary) -> bool: return not state.get("busy", true) and state.get("rows_current", false))
	_diagnostics = ""


## **The editor's planning wire, through the panel** (S3's `fixq-c`; the run sheet's set-ups and
## #183/#188's owed gaps, each red on `main`):
## * a duration chip is a number field typed in seconds, pre-filled with the gateway's own "120"
##   and its unit `s`, and "3" goes to `set_value_text` as typed (item 7: it was read-only);
## * a choice chip's menu draws the gateway's titles, never a value's JSON (item 6);
## * Queue a structure lists the Queue's own entries, each kind no queue takes locked with the
##   verifier's sentence (item 5: it listed Add a build target's);
## * the Remove list draws each target's line, the gateway's words (item 6: it drew JSON);
## * Add a rule, clicked, asks `new_rule`; Remove, clicked, sends one `remove` at the rule's
##   pointer (item 8: no control did either);
## * the picker lists the groups, and a group pick writes its skeleton (item 16);
## * a wall's Build target takes two ground clicks and the gateway's box (item 4).
func _check_the_planning_wire(stage: SubViewport, editor: CanvasLayer, bridge: Node, failures: Array[String]) -> void:
	bridge.editor_load(RIG_TEXT.to_utf8_buffer())
	if not await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false) and state.get("vocabulary", {}).get("known", false)):
		failures.append("the planning wire: the rule list never took an edit")
		return
	await _settle()
	var popup: PopupPanel = editor._number_popup
	# A duration, typed in seconds.
	var timeout := _chip_index(bridge, "/route/0/timeout_ms")
	var chip: Dictionary = bridge.editor_state().get("rule_list", {}).get("chips", [])[timeout] if timeout >= 0 else {}
	if String(chip.get("editor", "")) != "number" or String(chip.get("display_unit", "")) != "s":
		failures.append("the duration chip is not a number typed in seconds: %s" % [chip])
	else:
		editor._on_rule_chip(timeout)
		await _settle()
		if editor._number_field.field.text != "120" or editor._number_field.unit.text != "s":
			failures.append("the duration's field shows `%s %s`, not the gateway's `120 s`" % [editor._number_field.field.text, editor._number_field.unit.text])
		await _type_in_popup(stage, editor, "3")
		_typed.clear()
		_click(stage, Vector2(popup.position) + editor._number_field.send.get_global_rect().get_center())
		await _rig_until(bridge, func(state: Dictionary) -> bool: return not state.get("busy", true) and not _typed.is_empty())
		if _typed != ["3"]:
			failures.append("the duration's \"3\" did not go to `set_value_text` as typed: %s" % [_typed])
		popup.hide()
		await _settle()
	# Past the chip's bounds: the client compares nothing, so the gateway is asked and its refusal
	# is said in the popup (fix pass 1, item 4).
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	var pct := _chip_index(bridge, "/cmdr_hp_pct/pct")
	editor._on_rule_chip(pct)
	await _settle()
	await _type_in_popup(stage, editor, "150")
	_typed.clear()
	_click(stage, Vector2(popup.position) + editor._number_field.send.get_global_rect().get_center())
	await _rig_until(bridge, func(state: Dictionary) -> bool: return String(state.get("field", {}).get("state", "")) == "refused")
	await _settle()
	if _typed != ["150"] or editor._number_said.text != Strings.text("rule_refused", {"detail": "INVALID_ARGUMENT: `150` is above the most this takes, `100` %"}):
		failures.append("a value past its bounds was not the gateway's to refuse: asked %s, said `%s`" % [_typed, editor._number_said.text])
	popup.hide()
	await _settle()
	# A paste past the gateway's message cap is not sent: the popup says so, and the seat
	# connection stays up (fix pass 1, item 1).
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	editor._on_rule_chip(pct)
	await _settle()
	_typed.clear()
	editor._number_field.field.text = "1".repeat(300000)
	_click(stage, Vector2(popup.position) + editor._number_field.send.get_global_rect().get_center())
	await _rig_until(bridge, func(state: Dictionary) -> bool: return String(state.get("field", {}).get("state", "")) == "refused")
	await _settle()
	if not _typed.is_empty() or not editor._number_said.text.begins_with("Not sent:") or int(bridge.watch_state().get("unsent_seat", 0)) < 1 or int(bridge.watch_state().get("drops_seat", -1)) != 0:
		failures.append("a paste past the message cap went out or was not said: asked %d, said `%s`, %s" % [_typed.size(), editor._number_said.text.left(80), bridge.watch_state()])
	popup.hide()
	await _settle()
	# A choice, drawn by its titles.
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	var priority := _chip_index(bridge, "/initial/priority")
	editor._on_rule_chip(priority)
	await _settle()
	var drawn: Array[String] = []
	for item in editor._choice_menu.item_count:
		drawn.append(editor._choice_menu.get_item_text(item))
	if drawn != ["low", "normal", "high"]:
		failures.append("the priority's menu draws %s, not the gateway's titles" % [drawn])
	editor._choice_menu.hide()
	# The Queue's own list, and the Remove list's lines, for the Build beacon b_01.
	bridge.editor_describe_beacon("b_01")
	if not await _rig_until(bridge, func(state: Dictionary) -> bool: return String(state.get("beacon_detail", {}).get("id", "")) == "b_01"):
		failures.append("the planning wire: b_01's detail never came")
		return
	editor._visit_beacon = "b_01"
	var vocabulary: Dictionary = bridge.editor_state().get("vocabulary", {})
	editor.open_entry_list(EditorPanel.ITEM_LIST_QUEUE, vocabulary.get(String(EditorPanel.ENTRY_LISTS[EditorPanel.ITEM_LIST_QUEUE][1]), []), Rect2(Vector2(200, 300), Vector2(180, 200)))
	await _settle()
	var queued: Array = vocabulary.get("queue_blueprints", [])
	var rows: Array = editor.entry_rows()
	var generator_locked := false
	for row in rows:
		var at := int(row.get_meta("entry"))
		if at < queued.size() and String(queued[at].get("name", "")) == "generator":
			generator_locked = row.has_meta("locked")
	if rows.size() != queued.size() or not generator_locked:
		failures.append("Queue a structure lists %d rows of the Queue's %d, the Generator locked %s" % [rows.size(), queued.size(), generator_locked])
	editor._entry_popup.hide()
	editor._visit_drawn = ""
	editor._fill_visit_menu(bridge.editor_state())
	var lines: Array[String] = []
	for submenu in editor._visit_submenus:
		if (submenu as PopupMenu).id_pressed.get_connections().size() > 0 and submenu.get_parent() == editor._visit_menu:
			for item in (submenu as PopupMenu).item_count:
				lines.append((submenu as PopupMenu).get_item_text(item))
	var targets: Array = bridge.editor_state().get("beacon_detail", {}).get("targets", [])
	for target in targets:
		var line := String(target.get("line", ""))
		if line == "" or "{" in line or not lines.has(line):
			failures.append("the Remove list does not draw the target's line `%s`: %s" % [line, lines])
	# Add a rule and Remove one, through their own buttons.
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	var add: Button = null
	var remove: Button = null
	for button in editor._rules_box.find_children("*", "Button", true, false):
		if (button as Button).has_meta("rule_add"):
			add = button
		elif (button as Button).has_meta("rule_remove") and remove == null:
			remove = button
	if add == null or remove == null:
		failures.append("the rule list has no Add a rule or no Remove button")
	else:
		var asked := _new_rules
		editor.panel_scroll().ensure_control_visible(add)
		await _settle()
		_click(stage, add.get_global_rect().get_center())
		await _rig_until(bridge, func(state: Dictionary) -> bool: return _new_rules > asked and not state.get("busy", true))
		if _new_rules != asked + 1:
			failures.append("Add a rule, clicked, asked `new_rule` %d times" % (_new_rules - asked))
		await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
		await _settle()
		for button in editor._rules_box.find_children("*", "Button", true, false):
			if (button as Button).has_meta("rule_remove"):
				remove = button
				break
		var rule := int(remove.get_meta("rule_remove"))
		var pointer := String(bridge.editor_state().get("rule_list", {}).get("rules", [])[rule].get("pointer", ""))
		_patches.clear()
		editor.panel_scroll().ensure_control_visible(remove)
		await _settle()
		_click(stage, remove.get_global_rect().get_center())
		await _rig_until(bridge, func(state: Dictionary) -> bool: return not _patches.is_empty() and not state.get("busy", true))
		var sent: Variant = JSON.parse_string(_patches[0]) if not _patches.is_empty() else null
		if not (sent is Array and sent.size() == 1 and String(sent[0].get("op", "")) == "remove" and String(sent[0].get("path", "")) == pointer):
			failures.append("Remove, clicked, sent %s, not one `remove` at %s" % [_patches, pointer])
	# A group from the picker.
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	var slots: Array = bridge.editor_state().get("rule_list", {}).get("slots", [])
	var slot := -1
	for index in slots.size():
		if slots[index].get("pickable", false):
			slot = index
			break
	editor.open_condition_picker(slot)
	await _settle()
	var group_row: TreeItem = null
	for row in editor._picker_tree.get_root().get_children():
		if row.get_text(0) == Strings.text("picker_groups"):
			group_row = row.get_child(0)
	if group_row == null or group_row.get_text(0) != "All of these":
		failures.append("the picker lists no group, or not as the gateway titles it")
	else:
		group_row.select(0)
		_patches.clear()
		editor._on_picker_use()
		await _rig_until(bridge, func(state: Dictionary) -> bool: return not _patches.is_empty() and not state.get("busy", true))
		if _patches.is_empty() or not ("\"all\"" in _patches[0] and "\"items\"" in _patches[0]):
			failures.append("a group pick sent %s, not the group's skeleton" % [_patches])
	editor._picker_popup.hide()
	# A wall: two ground clicks, and the gateway's box in the row.
	await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("rules_editable", false))
	await _settle()
	var blueprints: Array = vocabulary.get("blueprints", [])
	var wall := -1
	for index in blueprints.size():
		if String(blueprints[index].get("anchor_shape", "")) == "box":
			wall = index
	if wall < 0 or not bridge.editor_visit_row("b_01", "add_target", wall):
		failures.append("Add a build target took no wall: %s" % bridge.editor_state().get("status_key", ""))
		return
	_walls.clear()
	_patches.clear()
	bridge.editor_pick({"voxel": Vector3i(10, 10, 4)})
	if String(bridge.editor_state().get("status_key", "")) != "wall_second" or not bridge.editor_state().get("anchoring", false):
		failures.append("the wall's first corner did not ask for the second: %s" % bridge.editor_state().get("status_key", ""))
	bridge.editor_pick({"voxel": Vector3i(12, 11, 4)})
	await _rig_until(bridge, func(state: Dictionary) -> bool: return not _patches.is_empty() and not state.get("busy", true))
	if _walls.size() != 1 or not ("\"corner_a\"" in _walls[0] and "\"corner_b\"" in _walls[0]):
		failures.append("the two corners did not ask `compose_wall_box` once: %s" % [_walls])
	if _patches.is_empty() or not ("\"wall_box\"" in _patches[0] and "\"add_build_target\"" in _patches[0]):
		failures.append("the wall's row is not the gateway's target: %s" % [_patches])
	# The panel back at its top, as the checks after this one find it.
	editor.panel_scroll().scroll_vertical = 0
	await _settle()
	print("[client-check] the planning wire: a duration typed in seconds, titles, the Queue's list, target lines, Add and Remove rule, a group, a wall's box")


## The Ready line from the gateway's word on the seal (S3's `fixq-c`, item 13: the rig's reading of
## the seat's `get_status`, the one source): the filing sentence before a seal, and after the
## gateway accepted one this round, and said so on `get_status`, that Ready keeps it. The editor
## keeps no seal of its own, so its state names none.
func _check_the_ready_line_through_the_rig(bridge: Node, failures: Array[String]) -> void:
	_sealed = false
	await _rig_until(bridge, func(_state: Dictionary) -> bool: return bridge.watch_state().has("sealed_this_round"))
	if Lobby.ready_line(bridge.watch_state()) != Strings.text("lobby_ready_help"):
		failures.append("before a seal the Ready line says `%s`" % Lobby.ready_line(bridge.watch_state()))
	bridge.editor_submit()
	if not await _rig_until(bridge, func(_state: Dictionary) -> bool: return bridge.watch_state().get("sealed_this_round", false)):
		failures.append("an accepted seal never showed as this round's on the gateway's word")
	if Lobby.ready_line(bridge.watch_state()) != Strings.text("lobby_ready_sealed"):
		failures.append("after an accepted seal the Ready line says `%s`" % Lobby.ready_line(bridge.watch_state()))
	if bridge.editor_state().has("sealed_this_round"):
		failures.append("the editor still keeps a seal of its own beside the gateway's")


## Load, pressed through its own button, opens in the folder of the file the text came from.
func _check_load_through_its_button(stage: SubViewport, editor: CanvasLayer, bridge: Node, failures: Array[String]) -> void:
	var path := ProjectSettings.globalize_path("res://fixtures/needs_a_fix.jsonc")
	if not editor.load_file(path) or not await _rig_until(bridge, func(state: Dictionary) -> bool: return String(state.get("file", "")) == path):
		failures.append("the rig-backed editor never took `%s` as the text's file" % path)
		return
	editor._load_dialog.current_dir = ProjectSettings.globalize_path("res://scripts")
	var load: Button = null
	for button in editor._panel.find_children("*", "Button", true, false):
		if (button as Button).text == Strings.text("load"):
			load = button
	if load == null:
		failures.append("the panel has no Load button")
		return
	_click(stage, load.get_global_rect().get_center())
	await _settle()
	if editor._load_dialog.current_dir.trim_suffix("/") != path.get_base_dir().trim_suffix("/"):
		failures.append("Load, pressed, opens in `%s`, not the text's file's folder" % editor._load_dialog.current_dir)
	editor._load_dialog.hide()


## **The entry list scrolls when it is taller than the window** (fix pass 1, item 6): 40 entries
## in a window 600 pixels tall stay inside it, in a frame that scrolls.
func _check_the_tall_entry_list(stage: SubViewport, editor: CanvasLayer, failures: Array[String]) -> void:
	var kept := stage.size
	stage.size = Vector2i(OWNERS_WINDOW.x, 600)
	await _settle()
	var entries: Array = []
	for index in 40:
		entries.append({"name": "e%d" % index, "title": "Entry %d" % index, "locked": ""})
	editor.open_entry_list(EditorPanel.ITEM_LIST_QUEUE, entries, Rect2(Vector2(200, 300), Vector2(180, 200)))
	await _settle()
	var popup: PopupPanel = editor._entry_popup
	var rect := Rect2(Vector2(popup.position), Vector2(popup.size))
	print("[client-check] 40 entries in a 600 px window: %s" % rect)
	if not Rect2(Vector2.ZERO, Vector2(stage.size)).encloses(rect):
		failures.append("40 entries in a 600 px window run outside it: %s" % rect)
	if editor._entry_scroll.get_v_scroll_bar().max_value <= editor._entry_scroll.size.y:
		failures.append("40 entries in a 600 px window do not scroll")
	popup.hide()
	stage.size = kept
	await _settle()


## --- The rig-backed checks (fix pass 1, item 5) ---------------------------------------------
##
## These reach the Rust they pin: the bridge's own watch rig and editor, both connections open,
## and this script standing where the gateway would, answering each frame the rig sends with a
## result of the gateway's shape (`_answer`). A check may hold a frame back (`hold`), answer it
## later (`_release`), and drives the panel with input pushed into the stage.

## The Lull footer every answer carries.
const LULL_FOOTER := {"phase": "lull", "phase_remaining_ms": 180000, "round": 1}
## The playbook the rig-backed checks open: any text, since this stand-in answers every check.
const RIG_TEXT := "{\"declarative\":{\"route\":[]}}"

## Frames held back by a check, `[link, request]` each.
var _held: Array = []
## What the stand-in's `verify_plan` reports: diagnostics as JSON text, and whether it qualifies.
var _diagnostics := ""
## Every `patch_plan` the stand-in answered, its `json_patch` as sent.
var _patches: Array[String] = []
## Whether the stand-in accepted a submission: its `get_status` then says this seat is sealed.
var _sealed := false
## Every `set_value_text`'s `typed`, as sent; every `compose_wall_box` request's text; and how
## many `new_rule` calls the stand-in answered.
var _typed: Array[String] = []
var _walls: Array[String] = []
var _new_rules := 0


## Starts the bridge's watch rig with both connections open, opens RIG_TEXT in the editor, and
## answers until the editor holds it with its rule list rendered.
func _rig_begin(bridge: Node) -> bool:
	_held.clear()
	_patches.clear()
	_diagnostics = ""
	bridge.configure(4, MesherRules.RULES_JSON)
	bridge.watch_begin()
	bridge.watch_opened(0)
	bridge.watch_opened(1)
	bridge.editor_seat("seat.0")
	bridge.editor_load(RIG_TEXT.to_utf8_buffer())
	return await _rig_until(bridge, func(state: Dictionary) -> bool: return state.get("has_text", false) and state.get("prose_current", false) and state.get("rows_current", false))


## Answers the rig's frames until `done` holds of the editor state, or about four seconds pass.
## `hold` is called with each request's method; a true answer keeps the frame back in `_held`.
func _rig_until(bridge: Node, done: Callable, hold: Callable = Callable()) -> bool:
	for _frame in 240:
		for frame in bridge.watch_outbox():
			var link := int(frame[0])
			var text := String(frame[1])
			var request: Dictionary = JSON.parse_string(text)
			if hold.is_valid() and hold.call(String(request.get("method", ""))):
				_held.append([link, text])
				continue
			bridge.watch_receive(link, _answer(text, ""))
		if done.call(bridge.editor_state()):
			return true
		await get_tree().process_frame
	return false


## Answers the first held frame with the refusal `error` (a JSON-RPC error object as text), or
## with its usual result when `error` is "".
func _release(bridge: Node, error: String) -> void:
	if _held.is_empty():
		return
	var held: Array = _held.pop_front()
	bridge.watch_receive(int(held[0]), _answer(String(held[1]), error))


## The stand-in's answer to the request `text`, as a JSON-RPC frame's text: `error` when one is
## given, otherwise a result of the shape the gateway answers that method with, and the Lull
## footer. Written as text, never through JSON.stringify of parsed numbers, which GDScript holds
## as floats and would write as "1.0".
func _answer(text: String, error: String) -> String:
	var found := RegEx.create_from_string("\"id\": *([0-9]+)").search(text)
	var id := found.get_string(1) if found != null else "0"
	var head := "{\"jsonrpc\":\"2.0\",\"id\":%s," % id
	if error != "":
		return head + "\"error\":" + error + "}"
	var request: Dictionary = JSON.parse_string(text)
	var params: Dictionary = request.get("params", {})
	var footer := "\"_status\":{\"phase\":\"lull\",\"phase_remaining_ms\":180000,\"round\":1}"
	var body := ""
	match String(request.get("method", "")):
		"get_view":
			body = "\"next_cursor\":\"c1\",\"complete\":true,"
		"verify_plan":
			body = "\"report\":{\"qualifies\":true,\"depth\":\"%s\",\"diagnostics\":[%s]}," % [String(params.get("depth", "quick")), _diagnostics]
		"patch_plan":
			_patches.append(String(params.get("json_patch", "")))
			body = "\"playbook_jsonc\":%s,\"inverse_json_patch\":\"[]\"," % JSON.stringify(String(params.get("playbook_jsonc", "")) + " ")
		"render_plan":
			return _fixture("res://fixtures/rule_list_render.json", id)
		"get_vocabulary":
			return _fixture("res://fixtures/vocabulary.json", id)
		"submit_plan":
			body = "\"report\":{\"qualifies\":true,\"depth\":\"full\",\"diagnostics\":[]},\"accepted\":true,"
			_sealed = true
		"get_status":
			body = "\"preset\":\"custom\",\"sealed_this_round\":%s," % ("true" if _sealed else "false")
		"get_briefing":
			body = "\"notes\":\"\","
		"list_drafts":
			body = "\"drafts\":[],"
		"list_templates":
			body = "\"templates\":[],"
		"get_map_summary":
			body = "\"features\":[],"
		"get_beacon":
			# Seat 0's Build beacon `b_01`, with its targets; any other, the core.
			return _fixture("res://fixtures/beacon_build.json" if String(params.get("beacon_id", "")) == "b_01" else "res://fixtures/beacon_core.json", id)
		"set_value_text":
			# The gateway reads the typed text (S3's `fixq`): digits are written, anything else
			# refused in its words, as `set_value_text` refuses.
			var typed := String(params.get("typed", ""))
			_typed.append(typed)
			if not typed.is_valid_int():
				return head + "\"error\":" + _refusal("INVALID_ARGUMENT", "`%s` is not a number" % typed) + "}"
			# The bound is the gateway's (fix pass 1, item 4): a percentage past 100 is refused.
			if String(params.get("pointer", "")).ends_with("/pct") and typed.to_int() > 100:
				return head + "\"error\":" + _refusal("INVALID_ARGUMENT", "`%s` is above the most this takes, `100` %%" % typed) + "}"
			body = "\"playbook_jsonc\":%s,\"inverse_json_patch\":\"[]\"," % JSON.stringify(String(params.get("playbook_jsonc", "")) + " ")
		"new_rule":
			_new_rules += 1
			body = "\"playbook_jsonc\":%s,\"inverse_json_patch\":\"[]\",\"rule_pointer\":\"/declarative/handlers/1\",\"rule_id\":\"rule_2\"," % JSON.stringify(String(params.get("playbook_jsonc", "")) + " ")
		"compose_wall_box":
			_walls.append(text)
			body = "\"target\":{\"blueprint_id\":\"wall\",\"anchor\":{\"voxel\":{\"x\":10,\"y\":10,\"z\":4}},\"wall_box\":{\"min\":{\"x\":10,\"y\":10,\"z\":4},\"max\":{\"x\":12,\"y\":11,\"z\":6}}},"
		"estimate_route", "resolve_refs":
			return head + "\"error\":" + _refusal("NOT_FOUND", "the stand-in prices nothing") + "}"
	return head + "\"result\":{" + body + footer + "}}"


## A JSON-RPC error object as the gateway writes one: its own code under `data`.
func _refusal(code: String, message: String) -> String:
	return "{\"code\":-32000,\"message\":%s,\"data\":{\"code\":%s}}" % [JSON.stringify(message), JSON.stringify(code)]


## A committed gateway answer's text, its id set to `id`.
func _fixture(path: String, id: String) -> String:
	return RegEx.create_from_string("\"id\": *[0-9]+").sub(FileAccess.get_file_as_string(path), "\"id\":" + id)


## The rule-list chip whose pointer ends in `suffix`, by its index in the bridge's list, or -1.
func _chip_index(bridge: Node, suffix: String) -> int:
	var chips: Array = bridge.editor_state().get("rule_list", {}).get("chips", [])
	for index in chips.size():
		if String(chips[index].get("pointer", "")).ends_with(suffix):
			return index
	return -1


## **A ground menu whose ghost was refused from the start stays open** (fix pass 2 item 1, the L4
## fix's regression): a ground click whose preview the bridge refuses at once (here there is no
## rig, as in a Push the door refuses it) keeps its menu after a redraw with no ghost, as the
## menus always did (item 139 (2)); only a menu whose line was filled from a ghost that then went
## closes.
func _check_the_refused_ghost_line(editor: CanvasLayer, failures: Array[String]) -> void:
	editor._on_ground_clicked(Vector3i(4, 4, 4), Vector2(200, 200))
	if not editor.menu_open():
		failures.append("the ground menu did not open for the refused-ghost check")
		return
	editor._draw_ghost({})
	if not editor.menu_open():
		failures.append("a ground menu whose ghost was never asked closed at the next redraw")
	if EditorPanel.ghost_went(true, true, null) or not EditorPanel.ghost_went(true, true, Vector3i(4, 4, 4)):
		failures.append("ghost_went does not close exactly a menu whose ghost existed and went")
	editor.close_menu()


## **Every seat's tally at the match's end** (item 155 (3) 7): the gateway's prose, then one frame
## a seat, each number as it came; between rounds, the prose alone.
func _check_the_recap_words(failures: Array[String]) -> void:
	# The Ready line says what Ready will do now (S3's `fixp`, P6b finding 4).
	if Lobby.ready_line({}) != Strings.text("lobby_ready_asking"):
		failures.append("with the gateway's word not yet told the Ready line says `%s`" % Lobby.ready_line({}))
	if Lobby.ready_line({"sealed_this_round": false}) != Strings.text("lobby_ready_help"):
		failures.append("with nothing sealed the Ready line says `%s`" % Lobby.ready_line({"sealed_this_round": false}))
	if Lobby.ready_line({"sealed_this_round": true}) != Strings.text("lobby_ready_sealed"):
		failures.append("with this round's seal the Ready line says `%s`" % Lobby.ready_line({"sealed_this_round": true}))
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


## **After a failure the chooser comes back with a button its words name** (S3's `prob`, fix
## pass 1). The real lobby, with no host: its link's failure is played to it in the three ways
## a match can fail - a Resume refused before any announce, a resumed match that failed after
## its announce, and a new match that failed after its announce - and each time the failure is
## shown as it came, the chooser is shown with New match, Resume only when a line is remembered
## and the failure was not that line's refused Resume, and the chooser and the watch strip are
## never shown together. The remembered line the announces write is removed afterwards.
func _check_the_lobby_after_a_failure(failures: Array[String]) -> void:
	var lobby: Node = LobbyScene.instantiate()
	add_child(lobby)
	await get_tree().process_frame
	var cases := [
		{"name": "a Resume refused before its announce", "resuming": true, "announce": false, "resume": false},
		{"name": "a resumed match failed after its announce", "resuming": true, "announce": true, "resume": true},
		{"name": "a new match failed after its announce", "resuming": false, "announce": true, "resume": true},
	]
	for case in cases:
		lobby._started = true
		lobby._resuming = case["resuming"]
		lobby._chooser.visible = false
		lobby._line = "check\tline"
		if case["announce"]:
			lobby._on_announced("check")
			if lobby._resuming:
				failures.append("%s: the announce left `_resuming` set" % case["name"])
			if lobby._chooser.visible:
				failures.append("%s: the chooser shows beside the watch strip" % case["name"])
		var reason := "the host said no (%s)" % case["name"]
		lobby._on_failed(reason)
		await get_tree().process_frame
		var new_shown := false
		for child in lobby._chooser.get_children():
			if child is Button and child.visible and (child as Button).text == Lobby.new_match_text():
				new_shown = true
		if not lobby._chooser.visible or not new_shown:
			failures.append("%s: no chooser with New match after the failure" % case["name"])
		if lobby._strip.visible or lobby.editor.visible:
			failures.append("%s: the watch strip or the editor stays beside the chooser" % case["name"])
		if lobby._resume_button.visible != case["resume"]:
			failures.append("%s: Resume is %s" % [case["name"], "shown" if lobby._resume_button.visible else "hidden"])
		if not String(lobby._status.text).contains(reason):
			failures.append("%s: the failure is not shown as it came: `%s`" % [case["name"], lobby._status.text])
		if lobby._started:
			failures.append("%s: the lobby still counts a match as started" % case["name"])
	if FileAccess.file_exists(Lobby.REMEMBERED):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(Lobby.REMEMBERED))
	lobby.queue_free()
	await get_tree().process_frame


## **A save is said as the bridge's write answered** (S3's `fixq-c`, fix passes 3 to 5, item A).
## The write is the operating system's atomic replace, in Rust (`crates/client-gdext/src/write.rs`),
## whose every step and failure `tests/write.rs` holds on the three systems; this check holds what
## is Godot's: Save (one path, `save_file`) refused for a target that is a folder and for a missing
## folder (the same on every system), each said on the status line in its row's words, the old
## file's bytes and the editor's file as they were; a save that lands whole, a player's own file
## beside it untouched; and a `write_<name>` row in the string table for every failure the bridge
## can answer.
func _check_the_saves(editor: CanvasLayer, bridge: Node, failures: Array[String]) -> void:
	var folder := ProjectSettings.globalize_path("user://client_check_saves")
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		failures.append("the save check could not make %s" % folder)
		return
	var text: PackedByteArray = bridge.editor_bytes()
	if text.is_empty():
		failures.append("the save check found no text on screen")
		return
	var old := "the old file\n".to_utf8_buffer()
	var held := folder + "/held.jsonc"
	var players := held + ".bak"
	var players_bytes := "the player's own copy\n".to_utf8_buffer()
	var a_folder := folder + "/a_folder.jsonc"
	if not _plain_write(held, old, failures) or not _plain_write(players, players_bytes, failures) or DirAccess.make_dir_recursive_absolute(a_folder) != OK:
		failures.append("the save check could not set up %s" % folder)
		return
	var before: Dictionary = bridge.editor_state()
	for case in [["a target that is a folder", a_folder, "write_is_folder"], ["a folder that does not exist", folder + "/no_such_folder/x.jsonc", "write_missing_folder"]]:
		if editor.save_file(case[1]):
			failures.append("a save to %s went ahead" % case[0])
			continue
		var said: String = editor.status_text()
		var words := Strings.text(String(case[2]), {"path": "", "error": "", "step": ""}).get_slice("{", 0).strip_edges()
		if not said.begins_with(Strings.text("status_save_not_done", {"why": ""})) or not said.contains(String(TEXT_OF[case[2]])):
			failures.append("a save to %s is not said in its row's words (`%s`): `%s`" % [case[0], words, said])
		var after: Dictionary = bridge.editor_state()
		if after.get("file", "") != before.get("file", "") or after.get("last_file", "") != before.get("last_file", ""):
			failures.append("a refused save to %s moved the editor's file" % case[0])
		if FileAccess.get_file_as_bytes(held) != old:
			failures.append("a refused save to %s changed %s" % [case[0], held])
	if not editor.save_file(held):
		failures.append("a save to %s did not land: `%s`" % [held, editor.status_text()])
	elif FileAccess.get_file_as_bytes(held) != text:
		failures.append("a save to %s did not write the text on screen whole" % held)
	if String(bridge.editor_state().get("file", "")) != held:
		failures.append("a save that landed did not make %s the text's file" % held)
	if FileAccess.get_file_as_bytes(players) != players_bytes:
		failures.append("a save touched the player's own %s" % players)
	for name in bridge.file_failure_names():
		if not Strings.TEXT.has("write_" + name):
			failures.append("the string table has no words for the write failure `%s`" % name)
	for path in [held, players]:
		if FileAccess.file_exists(path) and DirAccess.remove_absolute(path) != OK:
			failures.append("the save check could not remove %s" % path)
	for path in [a_folder, folder]:
		if DirAccess.remove_absolute(path) != OK:
			failures.append("the save check could not remove %s" % path)


## The fixed part of each write row the save check looks for: the words after `{path}`.
const TEXT_OF := {
	"write_is_folder": "is a folder, so nothing was written",
	"write_missing_folder": "does not exist, so nothing was written",
}


## `bytes` written to `path` for a check's set-up, every Error checked.
func _plain_write(path: String, bytes: PackedByteArray, failures: Array[String]) -> bool:
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		failures.append("could not open %s to set a check up (error %d)" % [path, FileAccess.get_open_error()])
		return false
	var stored := file.store_buffer(bytes)
	file.close()
	if not stored:
		failures.append("could not write %s to set a check up" % path)
	return stored


## **The remembered line is whole or absent, and forgetting it is said when it fails** (S3's
## `fixq-c`, fix passes 3 to 5, items 1b and 1c). The real lobby, pointed at a file of the check's
## own: its reader takes an absent file, an empty one and a folder as no match; remembering writes
## the line whole through the bridge, its bytes exactly what `main`'s writer wrote (the line and a
## newline, by `store_string`), and a line that cannot be written (a folder in the way) is said and
## claims no match; forgetting removes the file, and a removal that fails (a folder in the way) is
## said. What the write does on disk is `tests/write.rs`'s.
func _check_the_remembered_line(failures: Array[String]) -> void:
	var folder := ProjectSettings.globalize_path("user://client_check_lobby")
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		failures.append("the remembered-line check could not make %s" % folder)
		return
	var path := folder + "/last_match.txt"
	var lobby: Node = LobbyScene.instantiate()
	lobby.remembered = path
	add_child(lobby)
	await get_tree().process_frame
	var line := "check-match\t0x1\t2\t0\t30000\t6\tcustom"
	var state := {"round": 1, "phase": "lull"}
	if lobby.remembered_line() != "":
		failures.append("an absent remembered file reads as `%s`" % lobby.remembered_line())
	if _plain_write(path, PackedByteArray(), failures) and lobby.remembered_line() != "":
		failures.append("an empty remembered file reads as `%s`" % lobby.remembered_line())
	if DirAccess.remove_absolute(path) != OK or DirAccess.make_dir_recursive_absolute(path) != OK:
		failures.append("the remembered-line check could not set up a folder at %s" % path)
	elif lobby.remembered_line() != "":
		failures.append("a folder at the remembered file reads as `%s`" % lobby.remembered_line())
	# Remembering into a folder: not written, said, and no match claimed.
	lobby._line = line
	lobby._remember()
	if not lobby.status_line(state).contains(Strings.text("lobby_not_remembered", {"why": ""})) or lobby.remembered_line() != "":
		failures.append("a line that could not be remembered is not said, or a match is claimed: `%s`" % lobby.status_line(state))
	# Forgetting a folder in the way: said, and not forgotten.
	lobby._forget()
	if lobby._forgotten or not lobby.status_line(state).contains(Strings.text("lobby_not_forgotten", {"why": ""})):
		failures.append("a remembered line that could not be forgotten is not said: `%s`" % lobby.status_line(state))
	if DirAccess.remove_absolute(path) != OK:
		failures.append("the remembered-line check could not remove the folder at %s" % path)
	# Remembering, whole, byte for byte what `main`'s writer wrote.
	lobby._remember()
	var mains := folder + "/mains_writer.txt"
	var file := FileAccess.open(mains, FileAccess.WRITE)
	if file == null or not file.store_string(line + "\n"):
		failures.append("the remembered-line check could not write main's bytes to %s" % mains)
	if file != null:
		file.close()
	if FileAccess.get_file_as_bytes(path) != FileAccess.get_file_as_bytes(mains) or lobby._remember_failed != "":
		failures.append("the remembered line's bytes are not main's: `%s` (%s)" % [FileAccess.get_file_as_string(path), lobby._remember_failed])
	if lobby.remembered_line() != line:
		failures.append("the remembered line reads back as `%s`" % lobby.remembered_line())
	# Forgetting, removed.
	lobby._forget()
	if FileAccess.file_exists(path) or not lobby._forgotten or lobby._forget_failed != "":
		failures.append("an ended match was not forgotten by removing its file (%s)" % lobby._forget_failed)
	for leftover in [path, mains]:
		if FileAccess.file_exists(leftover) and DirAccess.remove_absolute(leftover) != OK:
			failures.append("the remembered-line check could not remove %s" % leftover)
	if DirAccess.remove_absolute(folder) != OK:
		failures.append("the remembered-line check could not remove %s" % folder)
	lobby.queue_free()
	await get_tree().process_frame


## **The refusal names the limit** (fix pass 2, item 3; review 3's G3): once the editor is set up,
## the too-large words carry the shared cap the bridge passed through, never an empty `{limit}`.
func _check_the_limit_is_named(bridge: Node, failures: Array[String]) -> void:
	var cap := str(bridge.watch_transport_limits().get("message_bytes", ""))
	var said := Strings.text("status_call_too_large", {"detail": "5"})
	if cap == "" or Strings.message_limit != cap or not said.contains(cap) or said.contains("{limit}"):
		failures.append("the too-large words do not name the cap `%s`: `%s`" % [cap, said])


## **What a peer takes, and what it does past that** (S3's `fixq-c`, fix pass 3, item 3).
## Through a peer host_link.gd makes (`new_peer`, from the bridge's limits) and its own reader
## (`packets_of`): the worst view page `tests/transport.rs` builds arrives whole, as one message;
## a message one byte past the inbound buffer is never handed over, cut or whole, and Godot
## closes the peer with 1009.
func _check_the_transport(bridge: Node, failures: Array[String]) -> void:
	var limits: Dictionary = bridge.watch_transport_limits()
	# A missing size refuses the peer in words, never a default (fix pass 4).
	var missing: Dictionary = HostLink.new_peer({"outbound_bytes": limits.get("outbound_bytes")})
	if missing.has("peer") or not String(missing.get("refusal", "")).contains("inbound_bytes"):
		failures.append("a peer with no inbound size was made: %s" % missing)
	var page := _worst_page()
	if page.to_utf8_buffer().size() != WORST_PAGE_BYTES:
		failures.append("the worst view page is %d bytes here, not the %d tests/transport.rs builds" % [page.to_utf8_buffer().size(), WORST_PAGE_BYTES])
	var got: Dictionary = await _receive(limits, page)
	var texts: Array = got.get("texts", [])
	if texts.size() != 1 or String(texts[0]) != page:
		failures.append("the worst view page did not arrive whole: %d message(s), %s byte(s)" % [texts.size(), [] if texts.is_empty() else [String(texts[0]).length()]])
	var past := "x".repeat(int(limits.get("inbound_bytes", 0)) + 1)
	got = await _receive(limits, past)
	texts = got.get("texts", [])
	if not texts.is_empty():
		failures.append("a message past the inbound buffer was handed over (%d message(s), the first %d bytes)" % [texts.size(), String(texts[0]).length()])
	if got.get("state", -1) != WebSocketPeer.STATE_CLOSED or got.get("code", -1) != 1009:
		failures.append("a message past the inbound buffer did not close the peer with 1009: %s" % got)
	print("[client-check] the transport: the %d-byte worst page whole; past the %d-byte buffer, nothing handed over and the peer closed with %s (`%s`)" % [WORST_PAGE_BYTES, int(limits.get("inbound_bytes", 0)), got.get("code", -1), got.get("reason", "")])


## `text` sent once from a local server to a peer `host_link.gd` makes, read through its own
## reader: `texts` handed over, the peer's `state`, its close `code` and `reason`.
func _receive(limits: Dictionary, text: String) -> Dictionary:
	var server := TCPServer.new()
	if server.listen(0, "127.0.0.1") != OK:
		return {"texts": [], "state": -1, "code": -1, "reason": "no local port"}
	var made: Dictionary = HostLink.new_peer(limits)
	if not made.has("peer"):
		server.stop()
		return {"texts": [], "state": -1, "code": -1, "reason": String(made.get("refusal", ""))}
	var client: WebSocketPeer = made["peer"]
	if client.connect_to_url("ws://127.0.0.1:%d/" % server.get_local_port()) != OK:
		server.stop()
		return {"texts": [], "state": -1, "code": -1, "reason": "no connection"}
	var sender: WebSocketPeer = null
	var sent := false
	var texts: Array[String] = []
	for frame in 600:
		if sender == null and server.is_connection_available():
			sender = WebSocketPeer.new()
			sender.outbound_buffer_size = text.to_utf8_buffer().size() + 1024
			if sender.accept_stream(server.take_connection()) != OK:
				break
		if sender != null:
			sender.poll()
		client.poll()
		if not sent and sender != null and sender.get_ready_state() == WebSocketPeer.STATE_OPEN and client.get_ready_state() == WebSocketPeer.STATE_OPEN:
			if sender.send_text(text) != OK:
				break
			sent = true
		if client.get_ready_state() == WebSocketPeer.STATE_OPEN:
			texts.append_array(HostLink.packets_of(client))
		if not texts.is_empty() or (sent and client.get_ready_state() == WebSocketPeer.STATE_CLOSED):
			break
		await get_tree().process_frame
	var answer := {"texts": texts, "state": client.get_ready_state(), "code": client.get_close_code(), "reason": client.get_close_reason(), "sent": sent}
	server.stop()
	return answer


## The worst view page, as `crates/client-gdext/tests/transport.rs` builds it and as the gateway
## spells every answer on the wire (`rpc::render`: the canonical writer, two-space indent, one
## entry per line, a trailing newline, which Godot's own `JSON.stringify` with that indent writes
## byte for byte): four chunks of 65 535 bytes of runs, every other chunk of the map one run, and
## 2 000 entities of the committed golden's shape.
static func _worst_page() -> String:
	var chunks := []
	for at in 288:
		chunks.append({"origin": {"x": at * 32, "y": 352, "z": 32}, "voxels_rle": "A".repeat(87380 if at < 4 else 4)})
	var entity := {"id": "s_4000", "kind": "structure", "subtype": "mortar", "owner": "seat.0", "at": {"x": 365, "y": 30, "z": 36}, "hp": 1, "max_hp": 400, "building": true, "look": {"shape": "dish", "x": 1, "y": 1, "z": 1}, "title": "Mortar, going up"}
	var entities := []
	for index in 2000:
		entities.append(entity)
	var page := {"jsonrpc": "2.0", "id": 999999, "result": {"at_ms": 480000, "chunks": chunks, "entities": entities, "next_cursor": "c".repeat(64), "complete": true}}
	return JSON.stringify(page, "  ", false) + "\n"


## **A close on a message is said, with the connection's state** (fix passes 3 to 5, item 3).
## A 1009 with no call in flight carried nothing, so nothing is said as closed on a message; the
## rig's reconnect policy has the link opened again at once (its first retry); and the words for a
## close the rig did record (on the map view or an editor call, `tests` in `rig.rs`) name the code
## and the connection's state, as the bridge's `closed_on` gives them.
func _check_a_close_on_a_message_is_said(bridge: Node, failures: Array[String]) -> void:
	bridge.watch_dropped(1, 1009)
	if bridge.watch_state().has("closed_on"):
		failures.append("a 1009 close that carried no call is said as a close on a message: %s" % bridge.watch_state().get("closed_on"))
	if String(bridge.watch_link_action(1)) != "connect":
		failures.append("the seat link's first reconnect is not due at once: %s" % bridge.watch_link_action(1))
	bridge.watch_opened(1)
	var closed := {"connection": "seat", "code": 1009, "call": "view", "open": false}
	var said := Lobby.closed_on_line({"closed_on": closed})
	if not said.contains("1009") or not said.contains(Strings.text("link_reopening")) or not said.contains(Strings.text("call_word_view")):
		failures.append("a 1009 close is not said with its code, its call and the connection's state: `%s`" % said)
	closed["open"] = true
	said = Lobby.closed_on_line({"closed_on": closed})
	if not said.contains(Strings.text("link_open")):
		failures.append("a connection open again after a close on a message is not said so: `%s`" % said)


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


## **Playtest mode in the real lobby** (S3's `prob`, part 2; the plan's E10): the chooser's check
## button is off at launch and the bridge's recorder with it; switched on, the line says so; the
## window's focus reaches the recorder through the scene tree's own notification and a camera move
## through the rig's `moved`; Export playtest bundle writes exactly one file, at the path chosen,
## whose entries Godot's own zip reader lists as the bundle's four and whose log and metrics carry
## what was forwarded; an export to a folder that is not there writes nothing and is said; and
## switched off, the recorder holds nothing.
func _check_the_playtest_lobby(failures: Array[String]) -> void:
	var lobby: Node = LobbyScene.instantiate()
	add_child(lobby)
	await get_tree().process_frame
	var playtest: Node = lobby.get_node_or_null("Playtest")
	var bridge: Node = lobby.vista.bridge
	var toggle: CheckButton = null
	for child in lobby._chooser.get_children():
		if child is CheckButton and (child as CheckButton).text == Strings.text("playtest_toggle"):
			toggle = child
	if playtest == null or toggle == null:
		failures.append("the lobby has no Playtest mode check button in its chooser")
		lobby.queue_free()
		return
	if toggle.button_pressed or bridge.playtest_state().get("on", true):
		failures.append("Playtest mode is on at launch: %s" % bridge.playtest_state())
	toggle.button_pressed = true
	await get_tree().process_frame
	if not bridge.playtest_state().get("on", false) or not playtest._note.visible:
		failures.append("the check button did not switch Playtest mode on, or its line is hidden")
	get_tree().root.propagate_notification(NOTIFICATION_APPLICATION_FOCUS_OUT)
	get_tree().root.propagate_notification(NOTIFICATION_APPLICATION_FOCUS_IN)
	lobby.vista.rig.follow(null)
	var name: String = preload("res://scripts/playtest.gd").BUNDLE_NAME
	var folder := ProjectSettings.globalize_path("user://client_check_playtest")
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		failures.append("the Playtest check could not make %s" % folder)
		lobby.queue_free()
		return
	for old in DirAccess.get_files_at(folder):
		if DirAccess.remove_absolute(folder + "/" + old) != OK:
			failures.append("the Playtest check could not clear %s" % old)
	var path := folder + "/" + name
	# Review 1's probe: the answer typed one key at a time to the cap and one past it, then
	# Export. The whole export is refused, no file is written, and the words stay until the text
	# changes; no keystroke reached the recorder.
	var limit: int = int(bridge.playtest_state().get("survey_limit", 0))
	var typed := ""
	for i in limit + 1:
		typed += "a"
		playtest._wanted.text = typed
		playtest._wanted_changed()
	var refused: Dictionary = playtest.export_to(path)
	if refused.get("written", true) or String(refused.get("refusal", "")) != "survey_too_long":
		failures.append("an answer past its cap did not refuse the export: %s" % refused)
	if not DirAccess.get_files_at(folder).is_empty():
		failures.append("a refused export wrote a file: %s" % [DirAccess.get_files_at(folder)])
	var words := Strings.text("playtest_export_survey_too_long", {"limit": limit})
	if playtest._said.text != words:
		failures.append("the refusal is not said in the string table's words: `%s`" % playtest._said.text)
	await get_tree().process_frame
	if playtest._said.text != words:
		failures.append("the refusal's words went before the answer changed")
	# A 300-character answer, typed key by key, then Export: the log holds one survey line.
	typed = ""
	for i in 300:
		typed += "b"
		playtest._wanted.text = typed
		playtest._wanted_changed()
	if playtest._said.text != "":
		failures.append("the refusal's words stayed after the answer changed")
	var written: Dictionary = playtest.export_to(path)
	if not written.get("written", false):
		failures.append("Export playtest bundle did not write: %s" % written)
	if Array(DirAccess.get_files_at(folder)) != [name]:
		failures.append("the export did not write exactly one file where it was asked: %s" % [DirAccess.get_files_at(folder)])
	if not String(playtest._said.text).contains(path):
		failures.append("the export's line does not name the file the tester chose: `%s`" % playtest._said.text)
	var reader := ZIPReader.new()
	if reader.open(path) != OK:
		failures.append("Godot's zip reader cannot open the bundle")
	else:
		var entries: PackedStringArray = reader.get_files()
		if entries != PackedStringArray(bridge.playtest_state().get("entries", [])):
			failures.append("the bundle's entries are %s" % entries)
		var log := reader.read_file("diagnostics.log").get_string_from_utf8()
		if not (" focus_out " in log and " focus_in " in log and " playtest_on " in log):
			failures.append("the focus did not reach the diagnostics log: `%s`" % log)
		if log.count(" survey_answered ") != 1 or log.count(" export_failed SURVEY_TOO_LONG ") != 1:
			failures.append("the log does not hold one survey line and the one refusal: `%s`" % log.left(400))
		# 4 301 keystrokes were typed: none of them is a line (a handful of events are).
		if log.count("\n") > 20:
			failures.append("the log holds %d lines after typing: a keystroke reached it" % log.count("\n"))
		var survey: Variant = JSON.parse_string(reader.read_file("survey.json").get_string_from_utf8())
		if not (survey is Dictionary) or String((survey as Dictionary).get("wanted_to_change", {}).get("text", "")) != typed:
			failures.append("the bundle's survey is not the answer exported: %s" % survey)
		var metrics: Variant = JSON.parse_string(reader.read_file("metrics.json").get_string_from_utf8())
		if not (metrics is Dictionary) or int((metrics as Dictionary).get("camera_moves", {}).get("follow", 0)) != 1:
			failures.append("the camera rig's move did not reach the metrics: %s" % metrics)
		reader.close()
	var missing: Dictionary = playtest.export_to(folder + "/no such folder/" + name)
	if missing.get("written", true) or String(playtest._said.text) == "":
		failures.append("an export to a missing folder was not said as not written: %s" % missing)
	if Array(DirAccess.get_files_at(folder)) != [name]:
		failures.append("a refused export left a file: %s" % [DirAccess.get_files_at(folder)])
	toggle.button_pressed = false
	await get_tree().process_frame
	if bridge.playtest_state().get("on", true) or bridge.playtest_state().has("pushes") or playtest._note.visible:
		failures.append("switched off, Playtest mode still holds or shows something: %s" % bridge.playtest_state())
	if DirAccess.remove_absolute(path) != OK or DirAccess.remove_absolute(folder) != OK:
		failures.append("the Playtest check could not remove its bundle")
	lobby.queue_free()
	await get_tree().process_frame
	print("[client-check] Playtest mode: off at launch, on by its button, focus and a camera move forwarded, one bundle written and read back, a missing folder said, off and cleared")


## A stand-in for the bridge that counts every Playtest call made to it.
class CountingBridge extends Node:
	var calls := 0
	var on := false

	func playtest_set(wanted: bool, _focused: bool) -> bool:
		calls += 1
		on = wanted
		return on

	func playtest_state() -> Dictionary:
		calls += 1
		return {"on": on, "phase": "", "survey_limit": 4000}

	func playtest_focus(_focused: bool) -> void:
		calls += 1

	func playtest_camera(_kind: String) -> bool:
		calls += 1
		return true

	func playtest_export(_path: String, _another: String, _understood: String, _text: String) -> Dictionary:
		calls += 1
		return {"written": false, "refusal": "off"}


## **Playtest mode off costs nothing** (fix pass 1, item 5): with the check button off, frames,
## focus changes and camera moves make no bridge call at all; switched on, they do.
func _check_playtest_off_costs_nothing(failures: Array[String]) -> void:
	# The stand-in answers every bridge method playtest.gd calls, read from its source: a new
	# call is a failure naming it, never a script error that counts nothing.
	var source := FileAccess.get_file_as_string("res://scripts/playtest.gd")
	var called := RegEx.create_from_string("bridge\\.([a-z_]+)\\(")
	var probe := CountingBridge.new()
	for found in called.search_all(source):
		if not probe.has_method(found.get_string(1)):
			failures.append("playtest.gd calls bridge.%s, which the counting stand-in does not count" % found.get_string(1))
	probe.free()
	var counting := CountingBridge.new()
	add_child(counting)
	var chooser := HBoxContainer.new()
	var column := VBoxContainer.new()
	add_child(chooser)
	add_child(column)
	var playtest: Node = preload("res://scripts/playtest.gd").new()
	add_child(playtest)
	playtest.setup(counting, null, chooser, column)
	for i in SETTLE_FRAMES:
		await get_tree().process_frame
	get_tree().root.propagate_notification(NOTIFICATION_APPLICATION_FOCUS_OUT)
	get_tree().root.propagate_notification(NOTIFICATION_APPLICATION_FOCUS_IN)
	playtest.camera_moved("zoom")
	if counting.calls != 0:
		failures.append("Playtest mode off made %d bridge call(s)" % counting.calls)
	playtest._toggle.button_pressed = true
	for i in SETTLE_FRAMES:
		await get_tree().process_frame
	if counting.calls < SETTLE_FRAMES:
		failures.append("Playtest mode on made %d bridge call(s) in %d frames" % [counting.calls, SETTLE_FRAMES])
	playtest.queue_free()
	counting.queue_free()
	chooser.queue_free()
	column.queue_free()
	await get_tree().process_frame
	print("[client-check] Playtest mode off: no bridge call in %d frames, a focus change and a camera move" % SETTLE_FRAMES)


## **A lost release does not swallow the next turn's count** (fix pass 1, item 6): a right-button
## drag says `turn` once; after the window loses focus with the button still held, the next drag
## says it again; and a new press starts a new turn even with no release between.
func _check_a_lost_release_counts_the_next_turn(failures: Array[String]) -> void:
	var stage := SubViewport.new()
	stage.size = OWNERS_WINDOW
	stage.own_world_3d = true
	stage.render_target_update_mode = SubViewport.UPDATE_DISABLED
	add_child(stage)
	var vista: Node3D = VistaScene.instantiate()
	stage.add_child(vista)
	vista.set_process(false)
	var rig: Node3D = vista.rig
	var turns: Array[String] = []
	rig.moved.connect(func(kind: String) -> void:
		if kind == "turn":
			turns.append(kind))
	var press := InputEventMouseButton.new()
	press.button_index = MOUSE_BUTTON_RIGHT
	press.pressed = true
	press.position = Vector2(400, 300)
	# The button held, as the input singleton reads it.
	Input.parse_input_event(press)
	await get_tree().process_frame
	var drag := InputEventMouseMotion.new()
	drag.button_mask = MOUSE_BUTTON_MASK_RIGHT
	drag.position = Vector2(410, 300)
	drag.relative = Vector2(10, 0)
	stage.push_input(press)
	stage.push_input(drag)
	stage.push_input(drag)
	if turns.size() != 1:
		failures.append("one drag said `turn` %d time(s)" % turns.size())
	# Focus lost with the button held: its release lands elsewhere.
	get_tree().root.propagate_notification(NOTIFICATION_APPLICATION_FOCUS_OUT)
	stage.push_input(drag)
	if turns.size() != 2:
		failures.append("the drag after a lost focus was not a new turn: %d" % turns.size())
	# A new press with no release between starts a new turn.
	stage.push_input(press)
	stage.push_input(drag)
	if turns.size() != 3:
		failures.append("the drag after a new press was not a new turn: %d" % turns.size())
	var release := press.duplicate()
	release.pressed = false
	Input.parse_input_event(release)
	await get_tree().process_frame
	stage.queue_free()
	await get_tree().process_frame
	print("[client-check] the camera's turn: said once a drag, again after a lost focus and after a new press")


## **A frame on a link no connection has is refused, and the rig is kept** (fix pass 2, item 7):
## `watch_receive` checks the link before it takes the rig.
func _check_a_bad_link_keeps_the_rig(failures: Array[String]) -> void:
	var vista: Node3D = VistaScene.instantiate()
	add_child(vista)
	vista.set_process(false)
	var bridge: Node = vista.bridge
	bridge.watch_begin()
	for link in [-1, 2, 7]:
		var answer: Dictionary = bridge.watch_receive(link, "{}")
		if String(answer.get("refused", "")) != "no_such_link":
			failures.append("a frame on link %d was not refused: %s" % [link, answer])
	if bridge.watch_state().is_empty():
		failures.append("a frame on a link no connection has dropped the rig")
	vista.queue_free()
	await get_tree().process_frame
	print("[client-check] a frame on a link no connection has: refused, the rig kept")


## **A stopped match can be exported** (fix pass 2, item 9): with Playtest mode on, the survey and
## Export show whenever no match is running (the chooser shown), hide while one runs, and show
## again once its host stops; the export then writes its bundle.
func _check_export_after_a_stopped_match(failures: Array[String]) -> void:
	var lobby: Node = LobbyScene.instantiate()
	add_child(lobby)
	await get_tree().process_frame
	var playtest: Node = lobby.get_node("Playtest")
	playtest._toggle.button_pressed = true
	await get_tree().process_frame
	if not playtest._survey.visible:
		failures.append("Export is not offered with Playtest on and no match running")
	# A match runs: the chooser is hidden, as the lobby hides it when a match starts.
	lobby._started = true
	lobby._chooser.visible = false
	await get_tree().process_frame
	if playtest._survey.visible:
		failures.append("Export is offered while a match runs")
	# Its host stops: the lobby says why and shows the chooser again.
	lobby._on_failed("the host stopped")
	await get_tree().process_frame
	await get_tree().process_frame
	if not playtest._survey.visible:
		failures.append("Export is not offered after the host stopped")
	var folder := ProjectSettings.globalize_path("user://client_check_playtest_stopped")
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		failures.append("the stopped-match check could not make %s" % folder)
	else:
		var path := folder + "/stopped.zip"
		var written: Dictionary = playtest.export_to(path)
		if not written.get("written", false):
			failures.append("a stopped match's bundle was not written: %s" % written)
		if FileAccess.file_exists(path) and DirAccess.remove_absolute(path) != OK:
			failures.append("the stopped-match check could not remove its bundle")
		if DirAccess.remove_absolute(folder) != OK:
			failures.append("the stopped-match check could not remove its folder")
	playtest._toggle.button_pressed = false
	lobby.queue_free()
	await get_tree().process_frame
	print("[client-check] Playtest mode: Export offered with no match running, hidden while one runs, offered and written after its host stopped")
