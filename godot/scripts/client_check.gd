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
# a duration, and no decision in this file — the one comparison it makes is
# `caught_panics == 0`, which is the assertion itself.
#
# Exit codes: 0 when everything held, 1 when it did not. The CI leg reads that and nothing
# else, because a Godot run that prints a failure and exits 0 is the silent-catch problem
# wearing a different hat.

extends Node3D

## Item 54's rules rows, as canonical `gp.v1.RulesTable` JSON.
##
## Inline rather than read from `rules/rules.v1.json`, because a `res://` path cannot
## escape the project folder and the rules table lives at the repository root. The bridge
## still parses it through the schema, which is the half being checked here.
##
## Being a COPY of a tuning row, it is pinned:
## `godot_project.rs::the_check_scripts_inline_rules_table_is_the_committed_one` parses this
## very literal and compares the mesher row with `rules/rules.v1.json`, so the copy cannot
## drift in silence (AGENTS.md section 12). `revision` is deliberately not compared — it is
## the whole table's version and moves whenever any lane adds a row anywhere in it.
const RULES_JSON := '{"revision":1,"mesher":{"surfacesPerFrame":4,"bytesPerFrame":524288,"ageFrames":2,"lightMax":15,"lightAtten":1}}'

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
	var configured: Dictionary = bridge.configure(4, RULES_JSON)
	if not configured.get("configured", false):
		failures.append("configure() refused the rules table: %s" % configured.get("reason", ""))
	else:
		print("[client-check] rules: %s" % configured.get("rules", {}))
		print("[client-check] renderer attached: %s (%s)" % [
			configured.get("attached", false), configured.get("reason", "")])

	# 3. A gateway result decodes through the schema, and a field the schema does not
	#    declare is refused rather than stripped.
	var status: Variant = bridge.decode_result("get_status", '{"status":{"phase":"LULL","round":1}}')
	if typeof(status) != TYPE_DICTIONARY:
		failures.append("decode_result(get_status) did not return a dictionary")
	else:
		print("[client-check] get_status: %s" % status)
	var refused: Variant = bridge.decode_result("get_status", '{"status":{"phase":"LULL"},"nope":1}')
	if refused != null:
		failures.append("an unknown field was accepted; it must be refused with a pointer")

	# 4. The chunk transposition, on a length the mesher would refuse.
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
