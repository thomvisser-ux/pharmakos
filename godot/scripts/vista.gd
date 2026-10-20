# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The vista: the terrain the view feed delivered, the things standing on it, under the
# Pall.
#
# Everything here is a view. The terrain is decoded, lit and meshed by the bridge
# (crates/client-gdext/src/view.rs) and uploaded to the RenderingServer under item 54's
# per-frame budget - this script only tells it where the camera is. The entities are the
# complete visible list the last completing page of the view carried: an id absent from
# it is gone, so the markers are replaced, never merged (decisions-log item 107 (5)).
#
# There is no visibility rule in this file, and no switch that shows more: the full-map
# unlock at elimination or match end is the gateway's policy change and arrives as
# ordinary view pages under the same token. The source-text test
# `crates/client-gdext/tests/unlock.rs` holds every script in godot/ to that.
#
# Entities are placeholder primitives until the art pass: PLACEHOLDER, every shape and
# colour below, OWNER at S6 (skeleton plan T16, PLACEHOLDERs). Positions arrive in whole
# voxels, so a marker eases towards its latest position. A unit the view gives a heading
# turns to it (the bridge hands the heading over as a draw yaw); anything else faces the
# way it last moved (walled floats; Tuning, OWNER).
#
# S2 (S2's plan, task `view`): shells, wrecks and ruins are markers of their own, and
# every entity the view gives hit points to carries a bar over it, filled by its `hp`
# over its `max_hp` exactly as the gateway sent them. What a seat sees of another seat's
# asset is the gateway's fog filter's (decision 5): an entity without `hp` draws no bar,
# never an empty one. A wreck and a ruin are nobody's (the wire's owner is ""), so they
# draw in nobody's colour. An id the view keeps but whose kind, subtype or owner changed
# is drawn afresh, because the marker's shape and colour are read from those three.

extends Node3D

const MesherRules := preload("res://scripts/mesher_rules.gd")

## How quickly a marker closes on its latest position, per second. PLACEHOLDER, Tuning.
const EASE_RATE := 6.0
## The smallest move that turns a marker to face along it, in voxels. PLACEHOLDER, Tuning.
const TURN_AFTER := 0.05

## Placeholder colours: own, another seat's, and nobody's. PLACEHOLDER, art, OWNER at S6.
const OWN_COLOUR := Color(0.20, 0.85, 0.80)
const OTHER_COLOUR := Color(0.90, 0.30, 0.25)
const NOBODY_COLOUR := Color(0.60, 0.60, 0.60)
## A beacon's emissive glow under the Pall. PLACEHOLDER, art, OWNER at S6.
const BEACON_GLOW := Color(1.0, 0.62, 0.20)
## A shell's glow in flight, and a wreck's and a ruin's darker body. PLACEHOLDER, art,
## OWNER at S6.
const SHELL_GLOW := Color(1.0, 0.85, 0.45)
const WRECK_COLOUR := Color(0.22, 0.20, 0.19)
const RUIN_COLOUR := Color(0.35, 0.33, 0.31)
## The hit-point bar: its size in voxels, how far over the marker's top it floats, and
## its two colours. PLACEHOLDER, art, OWNER at S6.
const BAR_SIZE := Vector2(2.4, 0.3)
const BAR_LIFT := 0.8
const BAR_BACK := Color(0.25, 0.05, 0.05)
const BAR_FILL := Color(0.30, 0.90, 0.35)

## The seat this client plays, as the gateway spells a seat: `seat.0`. Empty for a
## spectator. Own against other is `owner == my_seat` and nothing more.
var my_seat := ""

## Chunks still waiting on the drain queue after this frame's uploads; -1 before the
## first drain after a keyframe.
var pending := -1

## id -> {"node": Node3D, "target": Vector3, "kind": String, "subtype": String,
## "owner": String, "at": Vector3i, "yaw": float or null, "fill": MeshInstance3D or null}
var _markers := {}

@onready var bridge: Node = $Bridge
@onready var rig: Node3D = $CameraRig
@onready var _holder: Node3D = $Entities


## Reads the mesher's rules rows into the bridge. Returns whether it has a world to draw
## into.
func configure() -> bool:
	var report: Dictionary = bridge.configure(0, MesherRules.RULES_JSON)
	if not report.get("configured", false):
		printerr("[vista] configure refused: %s" % report.get("reason", ""))
	return report.get("attached", false)


## Takes what the bridge said about one view page. On a completing page the markers are
## replaced by its entity list.
func take_view(view: Dictionary) -> void:
	if view.has("entities"):
		_show(view["entities"], false)


## Places every marker at once, with no easing: for a still frame.
func settle_markers() -> void:
	for id in _markers:
		var marker: Dictionary = _markers[id]
		marker["node"].position = marker["target"]


## Looks at the whole map, once a keyframe has told the bridge how big it is.
func frame_whole_map() -> void:
	var extent: Vector3i = bridge.view_extent()
	if extent != Vector3i.ZERO:
		rig.frame_extent(Vector3(extent))


## This seat's commander marker, or null when the view shows none.
func my_commander() -> Node3D:
	for id in _markers:
		var marker: Dictionary = _markers[id]
		if marker["kind"] == "unit" and marker["subtype"] == "commander" and marker["owner"] == my_seat:
			return marker["node"]
	return null


## The voxel this seat's commander stands on, in the sim's axes, as the view last said;
## `Vector3i(-1, -1, -1)` when the view shows none.
func my_commander_at() -> Vector3i:
	for id in _markers:
		var marker: Dictionary = _markers[id]
		if marker["kind"] == "unit" and marker["subtype"] == "commander" and marker["owner"] == my_seat:
			return marker["at"]
	return Vector3i(-1, -1, -1)


## The marker drawn for the entity the view calls `id`, or null when the view shows none:
## where the editor marks a named Attack target.
func marker_node(id: String) -> Node3D:
	if not _markers.has(id):
		return null
	return _markers[id]["node"]


## How many markers are drawn.
func marker_count() -> int:
	return _markers.size()


## The beacons drawn: `id` (the `b_NN` id `get_beacon` takes), `owner`, and `position`, the
## middle of the marker in the world, for the editor to pick by screen distance.
func beacons() -> Array:
	var out := []
	for id in _markers:
		var marker: Dictionary = _markers[id]
		if marker["kind"] == "beacon":
			var node: Node3D = marker["node"]
			out.append({"id": id, "owner": marker["owner"], "position": node.global_position + Vector3(0.0, 4.5, 0.0)})
	return out


func _process(delta: float) -> void:
	var drained: Dictionary = bridge.view_drain(rig.camera.global_position)
	pending = int(drained.get("pending", pending))
	var weight := 1.0 - exp(-EASE_RATE * delta)
	for id in _markers:
		var marker: Dictionary = _markers[id]
		var node: Node3D = marker["node"]
		var goal: Vector3 = marker["target"]
		var step := goal - node.position
		if marker["yaw"] != null:
			node.rotation.y = lerp_angle(node.rotation.y, float(marker["yaw"]), weight)
		elif Vector2(step.x, step.z).length() > TURN_AFTER:
			node.rotation.y = lerp_angle(node.rotation.y, atan2(step.x, step.z), weight)
		node.position = node.position.lerp(goal, weight)


func _show(entities: Array, snap: bool) -> void:
	var seen := {}
	for entity in entities:
		var id := String(entity["id"])
		var at: Vector3i = entity["at"]
		# The sim's axes are x east, y north, z up; the world's are x, y up, z north. A
		# marker stands on the voxel it names, centred in it.
		var goal := Vector3(at.x + 0.5, at.z, at.y + 0.5)
		seen[id] = true
		if _markers.has(id) and not _same_look(_markers[id], entity):
			_markers[id]["node"].queue_free()
			_markers.erase(id)
		if not _markers.has(id):
			var node := _marker_for(entity)
			node.position = goal
			if entity.has("yaw"):
				node.rotation.y = float(entity["yaw"])
			_holder.add_child(node)
			_markers[id] = {
				"node": node,
				"kind": String(entity["kind"]),
				"subtype": String(entity["subtype"]),
				"owner": String(entity["owner"]),
				"target": goal,
				"at": at,
				"yaw": null,
				"fill": node.get_node_or_null("Bar/Fill"),
			}
		var marker: Dictionary = _markers[id]
		marker["target"] = goal
		marker["at"] = at
		marker["yaw"] = float(entity["yaw"]) if entity.has("yaw") else null
		_set_bar(marker, entity)
		if snap:
			marker["node"].position = goal
	for id in _markers.keys():
		if not seen.has(id):
			_markers[id]["node"].queue_free()
			_markers.erase(id)


## Whether a drawn marker still has the shape and colour `entity` asks for: its kind, its
## subtype and its owner are what `_marker_for` reads, and whether it has a bar.
func _same_look(marker: Dictionary, entity: Dictionary) -> bool:
	return (
		marker["kind"] == String(entity["kind"])
		and marker["subtype"] == String(entity["subtype"])
		and marker["owner"] == String(entity["owner"])
		and (marker["fill"] != null) == entity.has("hp")
	)


## Fills a marker's bar from the entity's `hp` over its `max_hp`, as the gateway sent them.
## The bridge refuses a page whose maximum is zero, so the division always has a divisor.
func _set_bar(marker: Dictionary, entity: Dictionary) -> void:
	var fill: MeshInstance3D = marker["fill"]
	if fill == null or not entity.has("hp"):
		return
	var share := float(entity["hp"]) / float(entity["max_hp"])
	var quad: QuadMesh = fill.mesh
	quad.size = Vector2(BAR_SIZE.x * share, BAR_SIZE.y)
	# Left-aligned in the billboard's own plane, so the bar empties towards its left end.
	quad.center_offset = Vector3(-BAR_SIZE.x * (1.0 - share) * 0.5, 0.0, 0.001)


func _marker_for(entity: Dictionary) -> Node3D:
	var kind := String(entity["kind"])
	var holder := String(entity["owner"])
	var colour := NOBODY_COLOUR
	if holder != "":
		colour = OWN_COLOUR if holder == my_seat else OTHER_COLOUR
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	var mesh: PrimitiveMesh = BoxMesh.new()
	var size := Vector3(1.0, 1.0, 1.0)
	if kind == "beacon":
		size = Vector3(2.0, 9.0, 2.0)
		material.emission_enabled = true
		material.emission = BEACON_GLOW
		material.emission_energy_multiplier = 2.5
	elif kind == "structure":
		size = Vector3(3.0, 3.0, 3.0)
	elif kind == "shell":
		var ball := SphereMesh.new()
		ball.radius = 0.4
		ball.height = 0.8
		mesh = ball
		size = Vector3(0.8, 0.8, 0.8)
		material.emission_enabled = true
		material.emission = SHELL_GLOW
		material.emission_energy_multiplier = 3.0
	elif kind == "wreck":
		size = Vector3(1.6, 0.5, 1.6)
		material.albedo_color = WRECK_COLOUR
	elif kind == "ruin":
		size = Vector3(3.0, 1.4, 3.0)
		material.albedo_color = RUIN_COLOUR
	elif kind == "unit" and String(entity["subtype"]) == "commander":
		size = Vector3(1.6, 2.6, 1.6)
	if mesh is BoxMesh:
		(mesh as BoxMesh).size = size
	mesh.material = material
	var body := MeshInstance3D.new()
	body.mesh = mesh
	body.position = Vector3(0.0, size.y * 0.5, 0.0)
	var node := Node3D.new()
	node.name = String(entity["id"])
	node.add_child(body)
	if kind == "unit":
		# The way it faces: a nose on its front, which is the marker's +z.
		var nose := MeshInstance3D.new()
		var tip := BoxMesh.new()
		tip.size = Vector3(0.3, 0.3, 0.5)
		tip.material = material
		nose.mesh = tip
		nose.position = Vector3(0.0, size.y * 0.6, size.z * 0.5 + 0.25)
		node.add_child(nose)
	if entity.has("hp"):
		node.add_child(_bar(size.y + BAR_LIFT))
	return node


## A hit-point bar `lift` voxels over the marker's feet: a back and a fill, both facing
## the camera.
func _bar(lift: float) -> Node3D:
	var bar := Node3D.new()
	bar.name = "Bar"
	bar.position = Vector3(0.0, lift, 0.0)
	for part in [["Back", BAR_BACK], ["Fill", BAR_FILL]]:
		var quad := QuadMesh.new()
		quad.size = BAR_SIZE
		var paint := StandardMaterial3D.new()
		paint.albedo_color = part[1]
		paint.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
		paint.billboard_mode = BaseMaterial3D.BILLBOARD_ENABLED
		paint.no_depth_test = false
		quad.material = paint
		var shown := MeshInstance3D.new()
		shown.name = part[0]
		shown.mesh = quad
		bar.add_child(shown)
	return bar
