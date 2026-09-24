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
# voxels and carry no heading, so a marker eases towards its latest position and faces the
# way it last moved (walled floats; Tuning, OWNER).

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

## The seat this client plays, as the gateway spells a seat: `seat.0`. Empty for a
## spectator. Own against other is `owner == my_seat` and nothing more.
var my_seat := ""

## Chunks still waiting on the drain queue after this frame's uploads; -1 before the
## first drain after a keyframe.
var pending := -1

## id -> {"node": Node3D, "target": Vector3, "kind": String, "subtype": String, "owner": String}
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
		if Vector2(step.x, step.z).length() > TURN_AFTER:
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
		if not _markers.has(id):
			var node := _marker_for(entity)
			node.position = goal
			_holder.add_child(node)
			_markers[id] = {
				"node": node,
				"kind": String(entity["kind"]),
				"subtype": String(entity["subtype"]),
				"owner": String(entity["owner"]),
				"target": goal,
				"at": at,
			}
		_markers[id]["target"] = goal
		_markers[id]["at"] = at
		if snap:
			_markers[id]["node"].position = goal
	for id in _markers.keys():
		if not seen.has(id):
			_markers[id]["node"].queue_free()
			_markers.erase(id)


func _marker_for(entity: Dictionary) -> Node3D:
	var kind := String(entity["kind"])
	var holder := String(entity["owner"])
	var colour := NOBODY_COLOUR
	if holder != "":
		colour = OWN_COLOUR if holder == my_seat else OTHER_COLOUR
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	var mesh := BoxMesh.new()
	var lift := 0.5
	if kind == "beacon":
		mesh.size = Vector3(2.0, 9.0, 2.0)
		lift = 4.5
		material.emission_enabled = true
		material.emission = BEACON_GLOW
		material.emission_energy_multiplier = 2.5
	elif kind == "structure":
		mesh.size = Vector3(3.0, 3.0, 3.0)
		lift = 1.5
	elif String(entity["subtype"]) == "commander":
		mesh.size = Vector3(1.6, 2.6, 1.6)
		lift = 1.3
	else:
		mesh.size = Vector3(1.0, 1.0, 1.0)
	mesh.material = material
	var body := MeshInstance3D.new()
	body.mesh = mesh
	body.position = Vector3(0.0, lift, 0.0)
	var node := Node3D.new()
	node.name = String(entity["id"])
	node.add_child(body)
	return node
