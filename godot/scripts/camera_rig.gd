# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The one camera rig the watch rig gets (spec section 3, During play): an own-fog camera
# with free-look and a follow-commander toggle.
#
# "Own-fog" is not something this script does. What the camera can show is what the
# gateway sent this seat's connection, and the gateway decides that (decisions-log item
# 107 (5)); the camera holds no visibility rule of its own and has no way to show more.
#
# Free-look: WASD or the arrow keys pan over the ground, Q and E lower and raise, the mouse
# wheel zooms, and the right mouse button held down turns the view. The lobby's Follow
# button (`lobby.gd`'s `_toggle_follow`) toggles following the commander; no key does. All
# of it is presentation - floats, easing and a camera - and none of it reaches the match.
#
# The keys are the camera's only while no GUI control has keyboard focus (decisions-log
# item 123 (2) 8): while the player types a note or a wizard value (or a file name in an
# embedded dialog), W, A, S, D, Q, E and the arrows are text, not camera moves. A mouse
# press on the 3D view that no control takes - a right-click, a wheel notch, or a
# left-click that `editor.gd` does not take as a beacon or ground click - releases that
# focus, so the keys come back without a restart. Godot 4.7 keeps a text field's focus
# through a click on empty space, which is why the rig releases it.

extends Node3D

## Pan speed over the ground, in voxels per second at the default zoom.
##
## PLACEHOLDER: camera speeds and easing are Tuning (walled floats) - OWNER; ruled at the
## demo to stand as they are for now (decisions-log item 126 (2) (b); register D-21).
const PAN_RATE := 90.0
## How much one wheel notch zooms, as a factor on the distance. PLACEHOLDER, as above.
const ZOOM_STEP := 1.12
## Radians of turn per pixel of mouse travel. PLACEHOLDER, as above.
const TURN_RATE := 0.005
## How quickly the camera closes on what it follows, per second. PLACEHOLDER, as above.
const FOLLOW_RATE := 4.0
## The nearest and farthest the camera sits from its focus, in voxels. PLACEHOLDER, as
## above.
const NEAREST := 12.0
const FARTHEST := 900.0

## The point the camera looks at.
var focus := Vector3.ZERO
## The heading, in radians about the vertical. The starting pose (this, the pitch and the
## distance) is a PLACEHOLDER, as above.
var yaw := 0.8
## The look-down angle, in radians; negative looks down. PLACEHOLDER, as above.
var pitch := -0.9
## How far the camera sits from its focus. PLACEHOLDER, as above.
var distance := 420.0
## What the camera follows, or null for free-look.
var target: Node3D = null

@onready var camera: Camera3D = $Camera


func _ready() -> void:
	_place()


## Looks at the whole of a map `extent` voxels across (x, up, north), from above its
## north-east corner (largest x and north nearest the camera).
func frame_extent(extent: Vector3) -> void:
	# A little past the centre towards the camera, so perspective does not cut the near
	# corner off the bottom of the frame.
	focus = Vector3(extent.x * 0.58, extent.y * 0.25, extent.z * 0.58)
	distance = maxf(extent.x, extent.z) * 1.5
	yaw = 0.8
	pitch = -1.0
	target = null
	_place()


## Starts or stops following `node`.
func follow(node: Node3D) -> void:
	target = node


func _process(delta: float) -> void:
	if target != null and is_instance_valid(target):
		var weight := 1.0 - exp(-FOLLOW_RATE * delta)
		focus = focus.lerp(target.global_position, weight)
	else:
		target = null
		var move := _key_move()
		if move != Vector3.ZERO:
			var scale_now := distance / 420.0
			focus += move.rotated(Vector3.UP, yaw) * PAN_RATE * scale_now * delta
	_place()


## The free-look keys held down, as a direction in the rig's frame; nothing while a GUI
## control has keyboard focus.
func _key_move() -> Vector3:
	var move := Vector3.ZERO
	if _keys_taken():
		return move
	if Input.is_key_pressed(KEY_W) or Input.is_key_pressed(KEY_UP):
		move.z -= 1.0
	if Input.is_key_pressed(KEY_S) or Input.is_key_pressed(KEY_DOWN):
		move.z += 1.0
	if Input.is_key_pressed(KEY_A) or Input.is_key_pressed(KEY_LEFT):
		move.x -= 1.0
	if Input.is_key_pressed(KEY_D) or Input.is_key_pressed(KEY_RIGHT):
		move.x += 1.0
	if Input.is_key_pressed(KEY_E):
		move.y += 1.0
	if Input.is_key_pressed(KEY_Q):
		move.y -= 1.0
	return move


## Whether a GUI control holds keyboard focus, in this viewport or in an embedded window
## open in it (the editor's Load and Save dialogs), so the keys are text, not moves.
func _keys_taken() -> bool:
	var viewport := get_viewport()
	if viewport.gui_get_focus_owner() != null:
		return true
	for window in viewport.get_embedded_subwindows():
		if window.gui_get_focus_owner() != null:
			return true
	return false


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.pressed:
		# A press on the view that no control took gives the keys back to the camera.
		get_viewport().gui_release_focus()
		if event.button_index == MOUSE_BUTTON_WHEEL_UP:
			distance = clampf(distance / ZOOM_STEP, NEAREST, FARTHEST)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			distance = clampf(distance * ZOOM_STEP, NEAREST, FARTHEST)
	elif event is InputEventMouseMotion and Input.is_mouse_button_pressed(MOUSE_BUTTON_RIGHT):
		yaw -= event.relative.x * TURN_RATE
		pitch = clampf(pitch - event.relative.y * TURN_RATE, -1.5, -0.1)


func _place() -> void:
	if camera == null:
		return
	var back := Vector3(0.0, 0.0, 1.0).rotated(Vector3.RIGHT, pitch).rotated(Vector3.UP, yaw)
	camera.global_position = focus + back * distance
	camera.look_at(focus, Vector3.UP)
